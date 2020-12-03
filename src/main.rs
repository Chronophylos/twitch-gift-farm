use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use flexi_logger::{style, DeferredNow, Record};
use lazy_static::lazy_static;
use log::{debug, error, info};
use messages::{SubPlan, UserNotice};
use ron::de::from_reader;
use serde::Deserialize;
use smol::{future::FutureExt, Timer};
use std::{fs::File, time::Duration};
use twitchchat::{
    connector::SmolConnectorTls,
    messages::{self, Commands, NoticeType},
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

        self.join_channels().await?;

        debug!("starting main loop");
        self.main_loop().await
    }

    async fn reconnect(&mut self) -> Result<()> {
        let connector = SmolConnectorTls::twitch()?;
        self.runner = AsyncRunner::connect(connector, &self.user_config).await?;

        self.join_channels().await
    }

    async fn join_channels(&mut self) -> Result<()> {
        info!("Joining {} channels", self.channels.len());
        let channels = self.channels.clone();

        for channel in channels {
            info!("Joining: {}", channel);
            if let Err(err) = self
                .join(&channel)
                .or(async {
                    Timer::after(Duration::from_secs(3)).await;
                    Err(anyhow!("timed out"))
                })
                .await
            {
                error!("Error while joining '{}': {}", channel, err);
            }

            // wait for 510 ms
            // max 20 join attempts per 10 seconds per user (2000 for verified bots)
            //Timer::after(Duration::from_millis(510)).await;
        }

        info!("Joined all channels");
        Ok(())
    }

    async fn join(&mut self, channel: &str) -> Result<()> {
        Ok(self.runner.join(channel).await?)
    }

    async fn main_loop(&mut self) -> Result<()> {
        loop {
            self.handle_message().await?;
        }
    }

    async fn handle_message(&mut self) -> Result<()> {
        match self.runner.next_message().await? {
            Status::Message(Commands::UserNotice(user_notice)) => {
                self.handle_user_notice(user_notice)
            }

            // stop if we're stopping
            Status::Quit => unreachable!("never quit"),

            Status::Eof => {
                info!("received an EOF, reconnecting");
                self.reconnect().await?;
            }

            // ignore the rest
            Status::Message(..) => {}
        }

        Ok(())
    }

    fn handle_user_notice(&self, msg: UserNotice<'_>) {
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

pub fn logger_format(
    w: &mut dyn std::io::Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();
    write!(
        w,
        "[{}] {} [{}] {}",
        now.now().format("%Y-%m-%d %H:%M:%S%.6f %:z"),
        style(level, level),
        record.module_path().unwrap_or("<unnamed>"),
        style(level, record.args())
    )
}

fn main() -> Result<()> {
    flexi_logger::Logger::with_env_or_str("info,twitch_gift_farm=trace")
        .format(logger_format)
        .start()?;

    let user_config = UserConfig::builder()
        .name(CONFIG.username.clone())
        .token(CONFIG.token.clone())
        .capabilities(&[Capability::Tags, Capability::Commands])
        .build()?;

    let mut bot = smol::block_on(Bot::new(user_config, CONFIG.channels.clone()))?;

    smol::block_on(bot.run())
}
