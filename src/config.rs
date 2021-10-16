use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BotConfig {
    pub blacklisted_users: Vec<String>,
    pub sasl_password: Option<String>,
    pub twitch_module: crate::twitch::config::Config,
}

impl BotConfig {
    pub fn from_path<P>(config_path: P) -> std::result::Result<BotConfig, serde_dhall::Error>
    where
        P: AsRef<Path>,
    {
        serde_dhall::from_file(config_path).parse::<BotConfig>()
    }
}
