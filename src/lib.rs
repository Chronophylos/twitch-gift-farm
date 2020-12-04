use anyhow::{Context, Result};
use directories::ProjectDirs;
use flexi_logger::{style, DeferredNow, Record};
use lazy_static::lazy_static;
use log::debug;
use ron::{
    de::from_reader,
    ser::{to_writer_pretty, PrettyConfig},
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config<'a> {
    pub username: Cow<'a, str>,
    pub token: Cow<'a, str>,
    pub channels: Vec<Cow<'a, str>>,
}

impl Config<'_> {
    pub fn load() -> Result<Self> {
        let path = Self::get_path();
        let file = File::open(path).context("Could not open config file")?;

        debug!("Loading config from {}", path.display());

        Ok(from_reader(file).context("Could not parse config file")?)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::get_path();
        let file = File::create(path).context("Could not open config file")?;

        debug!("Saving config to {}", path.display());

        Ok(to_writer_pretty(file, self, PrettyConfig::default())?)
    }

    fn get_path() -> &'static Path {
        lazy_static! {
            static ref PATH: PathBuf = ProjectDirs::from("com", "chronophylos", "twitch-gift-farm")
                .context("Could not get project dirs")
                .unwrap()
                .config_dir()
                .join("config.ron");
        }

        PATH.as_ref()
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
