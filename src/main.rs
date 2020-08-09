use anyhow::{Context, Result};
use directories::ProjectDirs;
use lazy_static::lazy_static;
use log::{debug, error, info, warn};
use messages::SubPlan;
use ron::de::from_reader;
use serde::Deserialize;
use std::{fs::File, sync::Arc};
use tokio::stream::StreamExt as _;
use twitchchat::{
    events,
    messages::{self, NoticeType},
    Capability, Control, Dispatcher, Runner, Status, UserConfig,
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
    control: Control,
}

impl Bot {
    async fn run(mut self, dispatcher: Dispatcher, channels: Vec<String>) {
        debug!("Running bot");
        info!("Will join {} channels", channels.len());

        let mut irc_ready = dispatcher.subscribe::<events::IrcReady>();
        let mut join = dispatcher.subscribe::<events::Join>();
        let mut notice = dispatcher.subscribe::<events::UserNotice>();
        let writer = self.control.writer();

        tokio::spawn(async move {
            let username = CONFIG.username.clone();
            while let Some(msg) = join.next().await {
                if msg.name == username {
                    info!("Joined {}", msg.channel);
                }
            }
        });

        tokio::spawn(async move {
            let username = CONFIG.username.clone();
            while let Some(msg) = notice.next().await {
                match msg.msg_id() {
                    Some(NoticeType::SubGift) | Some(NoticeType::AnonSubGift) => {
                        handle_subgift(msg, username.clone());
                    }
                    _ => {}
                }
            }
        });

        while let Some(msg) = irc_ready.next().await {
            info!("Connected as {}", msg.nickname);
            for channel in &channels {
                writer.join(channel).await.unwrap();
            }
        }
    }
}

fn handle_subgift(msg: Arc<messages::UserNotice>, username: String) {
    if let Some(recipient) = msg.msg_param_recipient_user_name() {
        if recipient != username {
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
        msg.channel,
        sub_plan_to_string(msg.msg_param_sub_plan()),
        gift_type,
        msg.display_name().or(msg.login()).unwrap(),
        msg.msg_param_sub_plan_name().unwrap().replace("\\s", " "),
    )
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

#[tokio::main]
async fn main() -> Result<()> {
    flexi_logger::Logger::with_env_or_str("info")
        .format(flexi_logger::colored_opt_format)
        .start()?;

    let dispatcher = Dispatcher::new();
    let (mut runner, control) = Runner::new(dispatcher.clone());

    // make a bot and get a future to its main loop
    let bot = Bot { control }.run(dispatcher, CONFIG.channels.clone());

    // connect to twitch
    // the runner requires a 'connector' factory so reconnect support is possible
    let connector = twitchchat::Connector::new(move || async move {
        let user_config = UserConfig::builder()
            .name(CONFIG.username.clone())
            .token(CONFIG.token.clone())
            .capabilities(&[Capability::Tags, Capability::Commands])
            .build()
            .unwrap();
        twitchchat::native_tls::connect(&user_config).await
    });

    // and run the dispatcher/writer loop
    let done = runner.run_to_completion(connector);

    // and select over our two futures
    tokio::select! {
        // wait for the bot to complete
        _ = bot => { info!("done running the bot") }
        // or wait for the runner to complete
        status = done => {
            match status {
                Ok(Status::Canceled) => { info!("runner was canceled") }
                Ok(Status::Eof) => { warn!("got an eof, exiting") }
                Ok(Status::Timeout) => { warn!("client timed out, exiting") }
                Err(err) => { error!("error running: {}", err) }
            }
        }
    }

    Ok(())
}
