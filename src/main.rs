use anyhow::{Context, Result};
use directories::ProjectDirs;
use lazy_static::lazy_static;
use log::{debug, error, info, trace};
use messages::{SubPlan, UserNotice};
use ron::de::from_reader;
use serde::Deserialize;
use std::fs::File;
use twitchchat::{
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
}

impl Bot {
    async fn run(&mut self, channels: Vec<String>) -> Result<()> {
        debug!("Running bot");
        info!("Will join {} channels", channels.len());

        let connector = SmolConnectorTls::twitch()?;
        let mut runner = AsyncRunner::connect(connector, &self.user_config).await?;

        info!("Connecting as {}", runner.identity.username());

        for channel in channels {
            info!("Joining: {}", channel);
            if let Err(err) = runner.join(&channel).await {
                error!("Error while joining '{}': {}", channel, err);
            }
        }

        debug!("starting main loop");
        self.main_loop(runner).await
    }

    async fn main_loop(&mut self, mut runner: AsyncRunner) -> Result<()> {
        let quit = runner.quit_handle();

        loop {
            // this drives the internal state of the crate
            match runner.next_message().await? {
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
                    let connector = SmolConnectorTls::twitch()?;
                    runner = AsyncRunner::connect(connector, &self.user_config).await?
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

        let gift_type = match msg.msg_id() {
            Some(NoticeType::SubGift) => "sub gift",
            Some(NoticeType::AnonSubGift) => "anonymous sub gift",
            _ => "unknown",
        };

        info!(
            "[{}] Received a {} {} from {}. Subscription Plan: {}",
            msg.channel(),
            sub_plan_to_string(msg.msg_param_sub_plan()),
            gift_type,
            msg.display_name().or(msg.login()).unwrap(),
            msg.msg_param_sub_plan_name().unwrap().replace("\\s", " "),
        )
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

    let mut bot = Bot { user_config };

    smol::block_on(bot.run(CONFIG.channels.clone()))
}
