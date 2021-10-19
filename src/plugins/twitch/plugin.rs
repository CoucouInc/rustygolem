use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use async_trait::async_trait;
use irc::client::prelude::Command;
use irc::proto::Message as IrcMessage;
use tokio::sync::mpsc;
use twitch_api2::{
    eventsub::stream::{StreamOfflineV1Payload, StreamOnlineV1Payload},
    helix::streams::{self, Stream},
    twitch_oauth2::AppAccessToken,
    types::Nickname,
    HelixClient,
};

use crate::plugin::{Plugin, Result};
use crate::plugins::twitch::{
    config::{Config, Message},
    webhook_server,
};

use crate::utils::parser;

struct WrappedToken(AppAccessToken);

impl WrappedToken {
    fn get(&self) -> &AppAccessToken {
        &self.0
    }
}

pub struct Twitch {
    config: Config,
    // If I share the same http client for getting the auth token and doing
    // twitch/helix operation, I get some horrible errors:
    //
    //    |
    // 36 |       async fn init() -> Result<Self> {
    //    |  _____________________________________^
    // 37 | |         Ok(init().await?)
    // 38 | |         // todo!()
    // 39 | |     }
    //    | |_____^ implementation of `twitch_api2::twitch_oauth2::client::Client` is not general enough
    //    |
    //    = note: `twitch_api2::twitch_oauth2::client::Client<'1>` would have to be implemented for the type `TwitchClient<'0, reqwest::Client>`, for any two lifetimes `'0` and `'1`...
    //    = note: ...but `twitch_api2::twitch_oauth2::client::Client<'2>` is actually implemented for the type `TwitchClient<'2, reqwest::Client>`, for some specific lifetime `'2`
    //
    // This seems to be a bug in rustc with regard to higher rank trait bound
    // with some info in this issue:
    // https://github.com/rust-lang/rust/issues/70263
    // and this doc:
    // https://doc.rust-lang.org/nomicon/hrtb.html
    // The whole thing seems to go away if the twitch client and the oauth client are kept
    // separate. Not the most elegant solution, but at least it works.
    client: HelixClient<'static, reqwest::Client>,

    // TODO wrap the uses of the token to automatically refresh it if expired
    token: WrappedToken,
    state: State,
}

#[derive(Debug, Default)]
pub struct State {
    // keys corresponding to Config.watched_streams
    // to identify which watched streams are currently online.
    online_streams: Arc<Mutex<HashMap<Nickname, Stream>>>,
}

impl State {
    fn add_streams(&self, streams: HashMap<Nickname, Stream>) {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .extend(streams)
    }

    fn add_stream(&self, nick: Nickname, stream: Stream) {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .insert(nick, stream);
    }

    fn remove_stream(&self, nick: &Nickname) -> Option<Stream> {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .remove(nick)
    }
}

#[async_trait]
impl Plugin for Twitch {
    async fn init() -> Result<Self> {
        let config = Config::from_file_keyed("golem_config.dhall")
            .context("Cannot read plugin_twitch.dhall")?;

        let auth_client = reqwest::Client::default();
        let client = HelixClient::new();

        let token = AppAccessToken::get_app_access_token(
            &auth_client,
            config.client_id.clone(),
            config.client_secret.clone(),
            vec![], // scopes
        )
        .await
        .context("Cannot get app access token")?;

        Ok(Twitch {
            config,
            token: WrappedToken(token),
            client,
            state: Default::default(),
        })
    }

    fn get_name(&self) -> &'static str {
        "twitch"
    }

    async fn run(&self, tx: mpsc::Sender<irc::proto::Message>) -> Result<()> {
        self.state.add_streams(self.get_live_streams().await?);

        let (twitch_tx, mut twitch_rx) = mpsc::channel(50);
        let consume_msg = || async move {
            while let Some(twitch_msg) = twitch_rx.recv().await {
                self.process_twitch_message(&tx, twitch_msg).await?;
            }
            Ok(())
        };

        try_join!(consume_msg(), webhook_server::run(&self.config, twitch_tx))?;
        Ok(())
    }

    async fn in_message(&self, msg: &IrcMessage) -> Result<Option<IrcMessage>> {
        self.in_message(msg).await
    }
}

impl Twitch {
    async fn process_twitch_message(
        &self,
        tx: &mpsc::Sender<irc::proto::Message>,
        msg: Message,
    ) -> Result<()> {
        log::debug!("Got a twitch message! {:?}", msg);
        match msg {
            Message::StreamOnline(online) => {
                self.stream_online(tx, online).await?;
            }

            Message::StreamOffline(offline) => {
                self.stream_offline(tx, offline).await?;
            }
        }
        Ok(())
    }

    async fn stream_online(
        &self,
        tx: &mpsc::Sender<irc::proto::Message>,
        online: StreamOnlineV1Payload,
    ) -> Result<()> {
        let target = self
            .config
            .watched_streams
            .iter()
            .find(|s| s.nickname == online.broadcaster_user_login);
        match target {
            None => log::warn!(
                "Got a notification for {} but not found in config",
                online.broadcaster_user_login
            ),
            Some(target) => {
                let nick = target.nickname.clone();
                let stream = self.get_live_stream(nick.clone()).await?;

                match stream {
                    None => log::info!(
                        "Got stream live notification but twitch returned nothing. TOCTOU :shrug:"
                    ),
                    Some(stream) => {
                        let url = format!("https://www.twitch.tv/{}", &target.nickname);
                        let game = &stream.game_name.to_string();
                        let game = if game.is_empty() {
                            "".to_string()
                        } else {
                            format!("({})", game)
                        };
                        let message = format!(
                            "Le stream de {} est maintenant live at {} ({})!",
                            target.nickname, url, game
                        );
                        self.state.add_stream(nick.clone(), stream);
                        for chan in &target.irc_channels {
                            tx.send(Command::PRIVMSG(chan.clone(), message.clone()).into())
                                .await
                                .with_context(|| format!("can't send message to {}", &chan))?;
                        }
                    }
                }
            }
        };
        Ok(())
    }

    async fn stream_offline(
        &self,
        tx: &mpsc::Sender<irc::proto::Message>,
        offline: StreamOfflineV1Payload,
    ) -> Result<()> {
        let target = self
            .config
            .watched_streams
            .iter()
            .find(|s| s.nickname == offline.broadcaster_user_login);
        match target {
            None => log::warn!(
                "Got a notification for {} but not found in config",
                offline.broadcaster_user_login
            ),
            Some(target) => {
                match self.state.remove_stream(&target.nickname) {
                    None => {
                        // this can happen when a streams goes online/offline rapidly,
                        // twitch only sends the offline event.
                        log::warn!("Got an offline notification for a stream not marked live");
                    }
                    Some(_s) => {
                        let message =
                                    format!("{} a arreté de streamer pour le moment. N'oubliez pas de like&subscribe.", target.nickname);
                        for chan in &target.irc_channels {
                            tx.send(Command::PRIVMSG(chan.clone(), message.clone()).into())
                                .await
                                .with_context(|| format!("can't send message to {}", &chan))?;
                        }
                    }
                }
            }
        };
        Ok(())
    }

    /// Returns a hashmap indexed by nickname and live stream information
    /// Abscence of a key indicates the stream is not live.
    async fn get_live_streams(&self) -> Result<HashMap<Nickname, Stream>> {
        let user_logins = self
            .config
            .watched_streams
            .iter()
            .map(|s| s.nickname.clone())
            .collect();
        let resp = self
            .client
            .req_get(
                streams::GetStreamsRequest::builder()
                    .user_login(user_logins)
                    .build(),
                self.token.get(),
            )
            .await
            .context("Can't get live stream")?;

        Ok(resp
            .data
            .into_iter()
            .map(|s| (s.user_login.clone(), s))
            .collect())
    }

    /// returning Ok(None) means the given nick isn't live atm
    pub async fn get_live_stream(&self, nick: Nickname) -> Result<Option<Stream>> {
        let mut resp = self
            .client
            .req_get(
                streams::GetStreamsRequest::builder()
                    .user_login(vec![nick.clone()])
                    .build(),
                self.token.get(),
            )
            .await
            .with_context(|| format!("Can't get live stream for {}", &nick))?;

        Ok(resp.data.pop())
    }

    async fn in_message(&self, msg: &IrcMessage) -> Result<Option<IrcMessage>> {
        let response_target = match msg.response_target() {
            None => return Ok(None),
            Some(target) => target,
        };

        if let Command::PRIVMSG(_source, privmsg) = &msg.command {
            if let Some(mb_target) = parser::single_command("streams", privmsg) {
                let prefix = mb_target.map(|t| format!("{}: ", t)).unwrap_or_default();
                let live_streams = self.state.online_streams.lock().expect("twitch state lock");
                let message = if live_streams.is_empty() {
                    format!("{}Y'a personne qui stream ici, çaynul !", prefix)
                } else {
                    format_streams(live_streams.values())
                };
                return Ok(Some(
                    Command::PRIVMSG(response_target.to_string(), message).into(),
                ));
            }
        }
        Ok(None)
    }
}

fn format_streams<'a, S>(streams: S) -> String
where
    S: Iterator<Item = &'a Stream>,
{
    streams.map(format_stream).collect::<Vec<_>>().join("−")
}

fn format_stream(stream: &Stream) -> String {
    let game = stream.game_name.to_string();
    let game = if game.is_empty() {
        format!("({})", game)
    } else {
        "".to_string()
    };
    format!(
        "{} {} started at {}",
        stream.user_name, game, stream.started_at
    )
}
