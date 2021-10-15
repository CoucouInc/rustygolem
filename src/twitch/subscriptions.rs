use anyhow::{Context, Result};
use futures::{StreamExt, TryStreamExt};

use twitch_api2::{
    eventsub::{
        stream::{StreamOfflineV1, StreamOnlineV1},
        EventSubscription, EventType,
    },
    helix::users::{get_users, User},
    twitch_oauth2::{AppAccessToken},
    types::{EventSubId, UserId},
    TwitchClient,
};

use crate::twitch::config::Config;
use twitch_api2::{eventsub, helix};

struct TwitchModule<'config> {
    config: &'config Config,
    token: AppAccessToken,
    /// users to watch for stream activity
    // known_users: Vec<User>,
    client: helix::HelixClient<'static, reqwest::Client>,
}

impl std::fmt::Debug for TwitchModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwitchModule")
            .field("config", &self.config)
            .field("token", &self.token)
            // .field("known_users", &self.known_users)
            .field("client", &"<HelixClient>")
            .finish()
    }
}

#[derive(Debug)]
struct Subscription {
    id: EventSubId,
    user_id: UserId,
    type_: EventType,
    status: eventsub::Status,
}

impl<'config> TwitchModule<'config> {
    pub async fn new_from_config(config: &'config Config) -> Result<TwitchModule<'config>> {
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

    async fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
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

        let users = if self.config.watched_streams.is_empty() {
            vec![]
        } else {
            let users_req = get_users::GetUsersRequest::builder()
                .login(
                    self.config
                        .watched_streams
                        .iter()
                        .map(|u| u.nickname.clone())
                        .collect(),
                )
                .build();
            self.client.req_get(users_req, &self.token).await?.data
        };

        let subs_to_delete: Vec<EventSubId> = subs
            .iter()
            .filter(|s| users.iter().find(|u| s.user_id == u.id).is_none())
            .map(|s| s.id.clone())
            .collect();

        futures::stream::iter(subs_to_delete)
            .map(Ok)
            .try_for_each_concurrent(5, |s| async move {
                self.delete_subscription(s).await?;
                Ok::<(), anyhow::Error>(())
            })
            .await?;

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

    async fn delete_subscription(&self, sub_id: EventSubId) -> Result<()> {
        log::info!("Deleting subscription with id {}", sub_id);
        self.client
            .req_delete(
                helix::eventsub::DeleteEventSubSubscriptionRequest::builder()
                    .id(sub_id)
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

pub async fn ensure_subscriptions(config: &Config) -> Result<()> {
    TwitchModule::new_from_config(config)
        .await?
        .sync_subscriptions()
        .await?;

    Ok(())
}
