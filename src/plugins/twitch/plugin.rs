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
    eventsub::{
        self,
        stream::{StreamOfflineV1, StreamOfflineV1Payload, StreamOnlineV1, StreamOnlineV1Payload},
        EventSubscription, EventType,
    },
    helix::{
        self,
        streams::{self, Stream},
        users::{get_users, User},
    },
    twitch_oauth2::AppAccessToken,
    types::{EventSubId, Nickname, UserId},
    HelixClient,
};

use crate::plugin::{Plugin, Result};
use crate::plugins::twitch::{
    config::{Config, Message},
    webhook_server,
};

use crate::utils::parser;
use futures::{StreamExt, TryStreamExt};

#[derive(Debug)]
pub struct Subscription {
    pub id: EventSubId,
    pub user_id: UserId,
    pub type_: EventType,
    pub status: eventsub::Status,
}

impl Subscription {
    fn is_valid(&self) -> bool {
        match self.status {
            eventsub::Status::Enabled | eventsub::Status::WebhookCallbackVerificationPending => {
                true
            }
            _ => false,
        }
    }
}

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
        self.sync_subscriptions().await?;
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
                self.on_stream_online(tx, online).await?;
            }

            Message::StreamOffline(offline) => {
                self.on_stream_offline(tx, offline).await?;
            }
        }
        Ok(())
    }

    async fn on_stream_online(
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
                            "Le stream de {} est maintenant live at {} {}!",
                            target.nickname, url, game
                        );
                        log::info!("Stream online: {}", &message);
                        self.state.add_stream(nick.clone(), stream);
                        for chan in &target.irc_channels {
                            let cmd = Command::PRIVMSG(chan.clone(), message.clone()).into();
                            log::info!("Stream online command to chan: {}, {:?}", &chan, &cmd);
                            tx.send(cmd)
                                .await
                                .with_context(|| format!("can't send message to {}", &chan))?;
                        }
                    }
                }
            }
        };
        Ok(())
    }

    async fn on_stream_offline(
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
                        log::info!("Stream offline: {}", &message);
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

    /// Make sure the bot is subscribed to stream.online and stream.offline
    /// for all the given user names (should not be capitalized)
    /// Also unsubscribe from existing subscriptions for user not listed in `user_names`
    async fn sync_subscriptions(&self) -> Result<()> {
        let subs = self.list_subscriptions().await?;

        let users = self
            .config
            .watched_streams
            .iter()
            .map(|u| &u.nickname)
            .collect::<Vec<_>>();
        log::info!("Syncing subscription for users {:?}", users);

        let users = self
            .get_users(
                self.config
                    .watched_streams
                    .iter()
                    .map(|u| u.nickname.clone())
                    .collect(),
                vec![],
            )
            .await?;

        let subs_to_delete: Vec<_> = subs
            .iter()
            .filter(|s| !s.is_valid() || !users.iter().any(|u| s.user_id == u.id))
            .collect();

        futures::stream::iter(subs_to_delete)
            .map(Ok)
            .try_for_each_concurrent(5, |s| async move {
                self.delete_subscription(s).await?;
                Ok::<(), anyhow::Error>(())
            })
            .await?;

        let subs = subs
            .into_iter()
            .filter(|s| s.is_valid())
            .collect::<Vec<_>>();

        futures::stream::iter(users)
            .map(Ok)
            .try_for_each_concurrent(5, |u| {
                let subs = &subs;
                async move {
                    self.sync_user_subscription(subs, u).await?;
                    Ok::<(), anyhow::Error>(())
                }
            })
            .await?;

        Ok(())
    }

    pub async fn get_users(&self, nicks: Vec<Nickname>, ids: Vec<UserId>) -> Result<Vec<User>> {
        if nicks.is_empty() && ids.is_empty() {
            return Ok(vec![]);
        }
        let req = get_users::GetUsersRequest::builder()
            .id(ids)
            .login(nicks)
            .build();
        let user_resp = self
            .client
            .req_get(req, self.token.get())
            .await
            .map_err(|e| crate::plugin::Error::Wrapped {
                source: Box::new(e),
                ctx: "cannot list subscriptions".to_string(),
            })?;

        Ok(user_resp.data)
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
        // TODO: handle pagination
        let resp = self
            .client
            .req_get(
                helix::eventsub::GetEventSubSubscriptionsRequest::builder().build(),
                self.token.get(),
            )
            .await
            .map_err(|e| crate::plugin::Error::Wrapped {
                source: Box::new(e),
                ctx: "cannot list subscriptions".to_string(),
            })?;
        // dbg!(&resp);

        let subs = resp
            .data
            .subscriptions
            .into_iter()
            .filter_map(|sub| {
                let status = sub.status;
                let typ = sub.type_;
                let id = sub.id;

                sub.condition
                    .as_object()
                    .and_then(|condition| condition.get("broadcaster_user_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| Subscription {
                        id,
                        user_id: UserId::new(s),
                        type_: typ,
                        status,
                    })
            })
            .collect::<Vec<_>>();

        Ok(subs)
    }

    async fn delete_subscription(&self, sub: &Subscription) -> Result<()> {
        log::info!("Deleting subscription {:?}", sub);
        self.client
            .req_delete(
                helix::eventsub::DeleteEventSubSubscriptionRequest::builder()
                    .id(sub.id.clone())
                    .build(),
                self.token.get(),
            )
            .await
            .map_err(|e| crate::plugin::Error::Wrapped {
                source: Box::new(e),
                ctx: format!("Failed to delete subscription {:?}", sub),
            })?;

        Ok(())
    }

    /// Ensure we're subscribed to the given user's stream.{online,offline} events
    async fn sync_user_subscription(&self, subs: &[Subscription], user: User) -> Result<()> {
        let sub_online = subs
            .iter()
            .find(|s| s.user_id == user.id && matches!(s.type_, EventType::StreamOnline));
        match sub_online {
            Some(_) => log::info!(
                "stream online subscription already exists for user_login {}",
                user.login
            ),
            None => {
                let event = StreamOnlineV1::builder()
                    .broadcaster_user_id(user.id.clone())
                    .build();
                self.subscribe(event).await.with_context(|| {
                    format!(
                        "failed to create stream.online subscription for (user_id, user_name) ({}, {})",
                        user.id, user.login
                    )
                })?;
                log::info!("Subscribed stream.online for channel {}", user.login);
            }
        };

        let sub_offline = subs
            .iter()
            .find(|s| s.user_id == user.id && matches!(s.type_, EventType::StreamOffline));
        match sub_offline {
            Some(_) => log::info!(
                "stream offline subscription already exists for user_login {}",
                user.login
            ),
            None => {
                let event = StreamOfflineV1::builder()
                    .broadcaster_user_id(user.id.clone())
                    .build();
                self.subscribe(event).await.with_context(|| {
                    format!(
                        "failed to create stream.offline subscription for (user_id, user_name) ({}, {})",
                        user.id, user.login
                    )
                })?;
                log::info!("Subscribed stream.offline for channel {}", user.login);
            }
        };

        Ok(())
    }

    /// Create a subscription. It will returns an error if the subscription
    /// already exists, so make sure to check for its existence or delete it
    /// before calling this function.
    /// This function returns once the subscription has been confirmed through
    /// the webhook, and requires the webhook server to be running in order to complete.
    async fn subscribe<E: EventSubscription + std::fmt::Debug + Clone>(
        &self,
        event: E,
    ) -> Result<()> {
        let sub_body = helix::eventsub::CreateEventSubSubscriptionBody::builder()
            .subscription(event.clone())
            .transport(
                eventsub::Transport::builder()
                    .method(eventsub::TransportMethod::Webhook)
                    .callback(self.config.callback_uri.0.clone())
                    .secret(self.config.app_secret.clone())
                    .build(),
            )
            .build();

        self.client
            .req_post(
                helix::eventsub::CreateEventSubSubscriptionRequest::builder().build(),
                sub_body,
                self.token.get(),
            )
            // treat a conflict as a crash there
            .await
            .map_err(|e| crate::plugin::Error::Wrapped {
                source: Box::new(e),
                ctx: format!("Failed to subscribe with event {event:?}"),
            })?;

        Ok(())
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
        "".to_string()
    } else {
        format!("({})", game)
    };
    let time_fmt = time::macros::format_description!("[hour]:[minute] [period]");
    let parsed = time::OffsetDateTime::parse(
        stream.started_at.as_str(),
        &time::format_description::well_known::Rfc3339,
    )
    .expect("valid RFC3339 timestamp for started_at");
    let started_at = parsed.format(time_fmt).unwrap();
    format!(
        "{} {} started at {started_at} (https://www.twitch.tv/{})",
        stream.user_name, game, stream.user_login
    )
}
