use serde::Deserialize;
use twitch_api2::{twitch_oauth2::{ClientId, ClientSecret}, types::Nickname};

#[derive(Debug, Deserialize, Clone)]
pub struct StreamSpec {
    /// nickname is the user_login, shown in the URL
    /// at www.twitch.tv/<user_id>
    pub nickname: Nickname,
    /// what is the irc nickname of the owner of that stream?
    pub irc_nick: String,
    /// Which channels to notify?
    pub irc_channels: Vec<String>,
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct Obfuscated(pub String);

impl std::fmt::Debug for Obfuscated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<Obfuscated string>")?;
        Ok(())
    }
}

impl std::clone::Clone for Obfuscated {
    fn clone(&self) -> Self {
        Obfuscated(self.0.clone())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub app_secret: String,
    pub watched_streams: Vec<StreamSpec>,
    pub webhook_bind: String,
    pub webhook_port: u16,
    pub callback_uri: Obfuscated,
    pub is_enabled: bool,
}
