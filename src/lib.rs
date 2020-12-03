use std::{borrow::Cow, fs::File};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use log::debug;
use ron::de::from_reader;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config<'a> {
    pub username: Cow<'a, str>,
    pub token: Cow<'a, str>,
    pub channels: Vec<Cow<'a, str>>,
}

impl Config<'_> {
    pub fn load() -> Result<Self> {
        let proj_dirs = ProjectDirs::from("com", "chronophylos", "twitch-gift-farm")
            .context("Could not get project dirs")?;

        let path = proj_dirs.config_dir().join("config.ron");

        debug!("Loading config from {}", path.display());

        Ok(
            from_reader(File::open(path).context("Could not open config file")?)
                .context("Could not parse config file")?,
        )
    }
}
