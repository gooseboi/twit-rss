use std::io::Read;
use clap::Parser;
use color_eyre::eyre::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct FetchConfig {
    pub max_links_per_fetch: usize,
    pub max_concurrent_users: usize,
    pub max_sessions_per_user: usize,
}

#[derive(Deserialize, Debug)]
pub struct DriverConfig {
    pub driver_count: usize,
    pub base_port: usize,
}

#[derive(Deserialize, Debug)]
pub struct TwitterConfig {
    pub auth_cache_fname: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(rename = "fetch")]
    pub fetch_config: FetchConfig,
    #[serde(rename = "drivers")]
    pub driver_config: DriverConfig,
    #[serde(rename = "twitter")]
    pub twitter_config: TwitterConfig,
}

impl Config {
    pub fn get() -> Result<Self> {
        #[derive(Parser, Debug)]
        struct CliConfig {
            #[arg(short, long, default_value = "config.toml")]
            config_path: String,
        }

        let cli_config = CliConfig::parse();

        let config_path = cli_config.config_path;
        let mut config = vec![];
        std::fs::File::open(config_path)
            .wrap_err("Could not open config file")?
            .read_to_end(&mut config)
            .wrap_err("Failed reading file")?;
        let config = String::from_utf8(config).wrap_err("Failed parsing config as UTF-8")?;
        let config: Config = toml::from_str(&config).wrap_err("Failed parsing config as TOML")?;

        Ok(config)
    }
}
