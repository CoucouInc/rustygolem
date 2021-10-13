use serde::Deserialize;
use twitch_api2::{twitch_oauth2::{ClientId, ClientSecret}, types::Nickname};

#[derive(Debug, Deserialize)]
pub struct StreamSpec {
    /// nickname is the user_login, shown in the URL
    /// at www.twitch.tv/<user_id>
    pub nickname: Nickname,
    /// what is the irc nickname of the owner of that stream?
    pub irc_nick: String,
    /// Which channels to notify?
    pub irc_channels: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub watched_streams: Vec<StreamSpec>,
}
