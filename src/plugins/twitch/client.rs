use std::collections::HashMap;

use anyhow::{Context, Result};
use futures::{StreamExt, TryStreamExt};

use twitch_api2::{
    eventsub::{
        stream::{StreamOfflineV1, StreamOnlineV1},
        EventSubscription, EventType,
    },
    helix::{
        streams::{self, Stream},
        users::{get_users, User},
    },
    twitch_oauth2::AppAccessToken,
    types::{EventSubId, Nickname, UserId},
    TwitchClient,
};

use crate::twitch::config::Config;
use twitch_api2::{eventsub, helix};

pub struct Client<C> {
    pub config: Config,
    pub token: AppAccessToken,
    /// users to watch for stream activity
    // known_users: Vec<User>,
    pub auth_client: C,
    pub client: helix::HelixClient<'static, reqwest::Client>,
}

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

impl<C> Client<C> {
    pub async fn new_from_config(config: Config) -> Result<Client> {
        let client: TwitchClient<reqwest::Client> = TwitchClient::default();
        let token = AppAccessToken::get_app_access_token(
            &client,
            config.client_id.clone(),
            config.client_secret.clone(),
            vec![], // scopes
        )
        .await?;

        Ok(Self {
            config,
            token,
            client: helix::HelixClient::default(),
        })
    }

    pub async fn get_users(&self, nicks: Vec<Nickname>, ids: Vec<UserId>) -> Result<Vec<User>> {
        if nicks.is_empty() && ids.is_empty() {
            return Ok(vec![]);
        }
        let req = get_users::GetUsersRequest::builder()
            .id(ids)
            .login(nicks)
            .build();
        Ok(self.client.req_get(req, &self.token).await?.data)
    }

    /// Returns a hashmap indexed by nickname and live stream information
    /// Abscence of a key indicates the stream is not live.
    pub async fn get_live_streams(&self) -> Result<HashMap<Nickname, Stream>> {
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
                &self.token,
            )
            .await?;

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
                    .user_login(vec![nick])
                    .build(),
                &self.token,
            )
            .await?;

        Ok(resp.data.pop())
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
        // TODO: handle pagination
        let resp = self
            .client
            .req_get(
                helix::eventsub::GetEventSubSubscriptionsRequest::builder().build(),
                &self.token,
            )
            .await?;
        // dbg!(&resp);

        let subs = resp
            .data
            .subscriptions
            .into_iter()
            .filter_map(|sub| {
                log::debug!("{:?}", sub);
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
            .filter(|s| !s.is_valid() || users.iter().find(|u| s.user_id == u.id).is_none())
            // .map(|s| s.id.clone())
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

    async fn delete_subscription(&self, sub: &Subscription) -> Result<()> {
        log::info!("Deleting subscription {:?}", sub);
        self.client
            .req_delete(
                helix::eventsub::DeleteEventSubSubscriptionRequest::builder()
                    .id(sub.id.clone())
                    .build(),
                &self.token,
            )
            .await?;

        Ok(())
    }

    /// Ensure we're subscribed to the given user's stream.{online,offline} events
    async fn sync_user_subscription(&self, subs: &[Subscription], user: User) -> Result<()> {
        let sub_online = subs
            .iter()
            .find(|s| s.user_id == user.id && matches!(s.type_, EventType::StreamOnline));
        match sub_online {
            Some(_) => println!(
                "stream online subscription already exists for user {:?}",
                user
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
                "stream offline subscription already exists for user {:?}",
                user
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
    async fn subscribe<E: EventSubscription>(&self, event: E) -> Result<()> {
        let sub_secret = std::env::var("TWITCH_APP_SECRET")
            .with_context(|| "TWITCH_APP_SECRET env var not found")?;
        let sub_body = helix::eventsub::CreateEventSubSubscriptionBody::builder()
            .subscription(event)
            .transport(
                eventsub::Transport::builder()
                    .method(eventsub::TransportMethod::Webhook)
                    .callback(self.config.callback_uri.0.clone())
                    .secret(sub_secret)
                    .build(),
            )
            .build();

        self.client
            .req_post(
                helix::eventsub::CreateEventSubSubscriptionRequest::builder().build(),
                sub_body,
                &self.token,
            )
            // treat a conflict as a crash there
            .await?;

        Ok(())
    }
}

pub async fn ensure_subscriptions(config: Config) -> Result<()> {
    Client::new_from_config(config)
        .await?
        .sync_subscriptions()
        .await?;

    Ok(())
}
