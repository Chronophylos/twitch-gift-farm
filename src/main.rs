use anyhow::{Context, Result};
use directories::ProjectDirs;
use lazy_static::lazy_static;
use log::{debug, error, info, trace};
use messages::{SubPlan, UserNotice};
use ron::de::from_reader;
use serde::Deserialize;
use smol::Timer;
use std::{fs::File, time::Duration};
use twitchchat::{
    commands,
    connector::SmolConnectorTls,
    messages::Commands,
    messages::{self, NoticeType},
    twitch::Capability,
    AsyncRunner, Status, UserConfig,
};

lazy_static! {
    static ref PROJ_DIRS: ProjectDirs =
        ProjectDirs::from("com", "chronophylos", "twitch-gift-farm").unwrap();
    static ref CONFIG: Config = {
        let path = PROJ_DIRS.config_dir().join("config.ron");
        debug!("Loading config from {}", path.display());
        from_reader(
            File::open(path)
                .context("Could not open config file")
                .unwrap(),
        )
        .context("Could not parse config file")
        .unwrap()
    };
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    username: String,
    token: String,
    channels: Vec<String>,
}

struct Bot {
    user_config: UserConfig,
    runner: AsyncRunner,
    channels: Vec<String>,
}

impl Bot {
    async fn new(user_config: UserConfig, channels: Vec<String>) -> Result<Self> {
        let connector = SmolConnectorTls::twitch()?;
        let runner = AsyncRunner::connect(connector, &user_config).await?;

        Ok(Self {
            user_config,
            channels,
            runner,
        })
    }

    async fn run(&mut self) -> Result<()> {
        debug!("Running bot");

        self.connect().await?;

        debug!("starting main loop");
        self.main_loop().await
    }

    async fn connect(&mut self) -> Result<()> {
        let connector = SmolConnectorTls::twitch()?;
        self.runner = AsyncRunner::connect(connector, &self.user_config).await?;

        info!("Connecting as {}", self.runner.identity.username());

        smol::spawn({
            let mut writer = self.runner.writer();
            let channels = self.channels.clone();

            info!("Will join {} channels", channels.len());

            async move {
                for channel in channels {
                    info!("Joining: {}", channel);
                    if let Err(err) = writer.encode(commands::join(&channel)).await {
                        error!("Error while joining '{}': {}", channel, err);
                    }

                    // wait for 510 ms
                    // max 20 join attempts per 10 seconds per user (2000 for verified bots)
                    Timer::after(Duration::from_millis(510)).await;
                }

                info!("Joined all channels");
            }
        })
        .detach();

        Ok(())
    }

    async fn main_loop(&mut self) -> Result<()> {
        let quit = self.runner.quit_handle();

        loop {
            // this drives the internal state of the crate
            match self.runner.next_message().await? {
                // if we get a Privmsg (you'll get an Commands enum for all messages received)
                Status::Message(Commands::UserNotice(user_notice)) => {
                    self.handle_subgift(user_notice)
                }
                // ignore the rest
                Status::Message(..) => continue,
                // stop if we're stopping
                Status::Quit => {
                    quit.notify().await;
                    break;
                }
                Status::Eof => {
                    self.connect().await?;
                }
            }
        }

        trace!("end of main loop");
        Ok(())
    }

    fn handle_subgift(&self, msg: UserNotice<'_>) {
        if let Some(recipient) = msg.msg_param_recipient_user_name() {
            if recipient != self.user_config.name {
                return;
            }
        }

        let gift_type = sub_gift_to_string(msg.msg_id());
        let sub_plan = sub_plan_to_string(msg.msg_param_sub_plan());
        let display_name = msg.display_name().or(msg.login()).unwrap_or("anonymous");
        let sub_plan_name = msg.msg_param_sub_plan_name().unwrap().replace("\\s", " ");

        info!(
            "[{}] Received a {} {} from {}. Subscription Plan: {}",
            msg.channel(),
            sub_plan,
            gift_type,
            display_name,
            sub_plan_name,
        )
    }
}

fn sub_gift_to_string(notice: Option<NoticeType>) -> &'static str {
    match notice {
        Some(NoticeType::SubGift) => "sub gift",
        Some(NoticeType::AnonSubGift) => "anonymous sub gift",
        _ => "unknown",
    }
}

fn sub_plan_to_string(plan: Option<SubPlan>) -> &'static str {
    match plan {
        Some(SubPlan::Prime) => "prime",
        Some(SubPlan::Tier1) => "tier1",
        Some(SubPlan::Tier2) => "tier2",
        Some(SubPlan::Tier3) => "tier3",
        _ => "Unknown",
    }
}

fn main() -> Result<()> {
    flexi_logger::Logger::with_env_or_str("info")
        .format(flexi_logger::colored_opt_format)
        .start()?;

    let user_config = UserConfig::builder()
        .name(CONFIG.username.clone())
        .token(CONFIG.token.clone())
        .capabilities(&[Capability::Tags, Capability::Commands])
        .build()?;

    let mut bot = smol::block_on(Bot::new(user_config, CONFIG.channels.clone()))?;

    smol::block_on(bot.run())
}
