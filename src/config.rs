use clap::Parser;
use color_eyre::eyre::{bail, eyre, Context, Result};
use serde::Deserialize;
use std::{collections::HashMap, env, io::Read};

#[derive(Deserialize, Debug)]
pub struct FetchConfig {
    pub max_links_per_fetch: usize,
    pub max_concurrent_users: usize,
    pub max_sessions_per_user: usize,
    pub fetch_username: String,
    pub max_retries: usize,
}

#[derive(Deserialize, Debug)]
pub struct DriverConfig {
    pub driver_count: usize,
    pub base_port: usize,
}

#[derive(Deserialize, Debug)]
pub struct TwitterConfig {
    pub auth_cache_fname: String,
    // This need to be there, to allow for auth, but they are options as a hack for toml to not
    // error out, and to not have two config structs.
    username: Option<String>,
    password: Option<String>,
    css_classes: HashMap<String, Vec<String>>,
    xpaths: HashMap<String, String>,
}

impl TwitterConfig {
    pub fn username(&self) -> &str {
        self.username.as_ref().unwrap()
    }

    pub fn password(&self) -> &str {
        self.password.as_ref().unwrap()
    }

    pub fn css_class(&self, name: &str) -> Result<Vec<&str>> {
        Ok(self
            .css_classes
            .get(name)
            .ok_or(eyre!("CSS class not in config"))?
            .iter()
            .map(|s| s.as_str())
            .collect())
    }

    pub fn xpath(&self, name: &str) -> Result<&str> {
        self
            .xpaths
            .get(name)
            .ok_or(eyre!("CSS class not in config"))
            .map(|s| s.as_str())
    }
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
            #[arg(short, long)]
            config_path: Option<String>,

            #[arg(short, long)]
            username: Option<String>,

            #[arg(short, long)]
            password: Option<String>,
        }

        let cli_config = CliConfig::parse();

        let config_path = if let Some(path) = cli_config.config_path {
            path
        } else if let Ok(path) = env::var("TWITARC_CONFIG") {
            path
        } else if let Ok(path) = env::var("TWITARC_DATA") {
            format!("{path}/config.toml")
        } else {
            "config.toml".to_owned()
        };

        let mut config = vec![];
        std::fs::File::open(config_path)
            .wrap_err("Could not open config file")?
            .read_to_end(&mut config)
            .wrap_err("Failed reading file")?;
        let config = String::from_utf8(config).wrap_err("Failed parsing config as UTF-8")?;
        let mut config: Config =
            toml::from_str(&config).wrap_err("Failed parsing config as TOML")?;

        if let Some(username) = cli_config.username {
            config.twitter_config.username = Some(username);
        } else if let Ok(username) = env::var("TWITTER_USERNAME") {
            config.twitter_config.username = Some(username);
        } else if config.twitter_config.username.is_none() {
            bail!("Could not load twitter username from CLI, env, nor config");
        }

        if let Some(password) = cli_config.password {
            config.twitter_config.password = Some(password);
        } else if let Ok(password) = env::var("TWITTER_PASSWORD") {
            config.twitter_config.password = Some(password);
        } else if config.twitter_config.password.is_none() {
            bail!("Could not load twitter password from CLI, env, nor config");
        }

        Ok(config)
    }
}
