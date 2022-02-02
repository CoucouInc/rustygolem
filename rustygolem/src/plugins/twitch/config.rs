use std::path::Path;

use serde::Deserialize;
use twitch_api2::{eventsub::stream::{StreamOfflineV1Payload, StreamOnlineV1Payload}, twitch_oauth2::{ClientId, ClientSecret}, types::Nickname};

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
}

// tmp struct to parse the config from a file with other stuff in it
#[derive(Deserialize)]
struct TC{twitch: Config}


impl Config {
    // pub fn from_file<P>(p: P) -> Result<Self, serde_dhall::Error>
    // where
    //     P: AsRef<Path>,
    // {
    //     Ok(serde_dhall::from_file(p).parse()?)
    // }

    /// read config from a file where it's under a key
    /// named "twitch"
    pub fn from_file_keyed<P>(p: P) -> Result<Self, serde_dhall::Error>
    where
        P: AsRef<Path>,
    {
        let tmp: TC = serde_dhall::from_file(p).parse()?;
        Ok(tmp.twitch)
    }

}


#[derive(Debug)]
pub enum Message {
    StreamOnline(StreamOnlineV1Payload),
    StreamOffline(StreamOfflineV1Payload),
}
