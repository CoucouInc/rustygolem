use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures::{StreamExt, TryStreamExt};

use twitch_api2::{
    eventsub::{
        stream::{StreamOfflineV1, StreamOnlineV1},
        EventSubscription, EventType,
    },
    helix::{
        users::{get_users, User},
        ClientRequestError, HelixClient, HelixRequestPostError,
    },
    twitch_oauth2::{AppAccessToken, ClientId, ClientSecret},
    types::{EventSubId, UserId, UserName},
    TwitchClient,
};

use twitch_api2::{eventsub, helix};

struct TwitchModule {
    client_id: ClientId,
    client_secret: ClientSecret,
    token: AppAccessToken,
    callback_uri: String,
    /// users to watch for stream activity
    known_users: Vec<User>,
    client: helix::HelixClient<'static, reqwest::Client>,
}

impl std::fmt::Debug for TwitchModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwitchModule")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret)
            .field("token", &self.token)
            .field("callback_uri", &self.callback_uri)
            .field("known_users", &self.known_users)
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

impl TwitchModule {
    async fn new(client_id: ClientId, client_secret: ClientSecret) -> Result<Self> {
        let client: TwitchClient<reqwest::Client> = TwitchClient::default();
        let token = AppAccessToken::get_app_access_token(
            &client,
            client_id.clone(),
            client_secret.clone(),
            vec![],
        )
        .await?;

        // TODO move that to the config file
        let callback_uri = "https://irc.geekingfrog.com/touitche/coucou".to_string();
        Ok(Self {
            client_id,
            client_secret,
            token,
            callback_uri,
            known_users: Vec::new(),
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
    async fn sync_subscriptions(&mut self, user_names: &[UserName]) -> Result<()> {
        let unknown_users = user_names
            .iter()
            .filter_map(
                |u| match self.known_users.iter().find(|ku| &ku.login == u) {
                    Some(_) => None,
                    None => Some(u),
                },
            )
            .cloned()
            .collect::<Vec<_>>();

        if !unknown_users.is_empty() {
            let usr_req = get_users::GetUsersRequest::builder()
                .login(unknown_users)
                .build();
            let users: Vec<User> = self
                .client
                .req_get(usr_req, &self.token)
                .await
                .with_context(|| "Get users for twitch modules")?
                .data;
            self.known_users = users;
        }

        self.sync_subscriptions_(user_names).await?;
        Ok(())
    }

    async fn sync_subscriptions_(&self, user_names: &[UserName]) -> Result<()> {
        println!("syncing subs for users: {:?}", user_names);
        let users: Vec<&User> = user_names
            .iter()
            .map(|user_name| {
                self.known_users
                    .iter()
                    .find(|u| &u.login == user_name)
                    .ok_or(anyhow!("Cannot find user with name {}", user_name))
            })
            .collect::<Result<Vec<_>>>()?;

        let existing_subs = self.list_subscriptions().await?;

        // delete subscriptions for users not specified
        futures::stream::iter(
            existing_subs
                .iter()
                .filter(|s| users.iter().find(|u| u.id == s.user_id).is_none())
                .map(Ok),
        )
        .try_for_each_concurrent(5, |s| async move {
            println!("deleting subscription {:?}", s);
            self.client
                .req_delete(
                    helix::eventsub::DeleteEventSubSubscriptionRequest::builder()
                        .id(s.id.clone())
                        .build(),
                    &self.token,
                )
                .await?;
            let r: Result<()> = Ok(());
            r
        })
        .await?;

        let existing_subs = Arc::new(existing_subs);
        futures::stream::iter(user_names)
            .map(Ok)
            .try_for_each_concurrent(5, |user_name| {
                let existing_subs = existing_subs.clone();
                async move {
                    let user = self
                        .known_users
                        .iter()
                        .find(|ku| &ku.login == user_name)
                        .unwrap();
                    self.sync_user_subscription(&existing_subs[..], user)
                        .await?;
                    let r: Result<()> = Ok(());
                    r
                }
            })
            .await?;

        Ok(())
    }

    async fn sync_user_subscription(&self, subs: &[Subscription], user: &User) -> Result<()> {
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
                println!("Subscribed stream.online for channel {}", user.login);
            }
        };

        let sub_offline = subs
            .iter()
            .find(|s| s.user_id == user.id && matches!(s.type_, EventType::StreamOffline));
        match sub_offline {
            Some(_) => println!(
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
                println!("Subscribed stream.offline for channel {}", user.login);
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
                    .callback(self.callback_uri.clone())
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

// TODO: add the list of users as param here
async fn ensure_subscriptions() -> Result<()> {
    let mut module = TwitchModule::new(
        ClientId::from(
            std::env::var("TWITCH_CLIENT_ID").with_context(|| "TWITCH_CLIENT_ID not found")?,
        ),
        ClientSecret::from(
            std::env::var("TWITCH_CLIENT_SECRET")
                .with_context(|| "TWITCH_CLIENT_SECRET not found")?,
        ),
    )
    .await?;

    // module.sync_subscriptions(&[]).await?;
    module.sync_subscriptions(&["geekingfrog".into()]).await?;

    Ok(())
}

// async fn test_subscribe<'a, C>(
//     helix_client: &'a HelixClient<'a, C>,
//     token: &AppAccessToken,
//     user: helix::users::User,
// ) -> Result<()>
// where
//     C: twitch_api2::HttpClient<'a>,
// {
//     let cb_uri = "https://irc.geekingfrog.com/touitche/coucou".to_string();
//     println!("callback uri for test subscription: {}", cb_uri);
//     let body = helix::eventsub::CreateEventSubSubscriptionBody::builder()
//         .subscription(
//             eventsub::stream::StreamOnlineV1::builder()
//                 .broadcaster_user_id(user.id.clone())
//                 .build(),
//         )
//         .transport(
//             eventsub::Transport::builder()
//                 .method(eventsub::TransportMethod::Webhook)
//                 .callback(cb_uri.clone())
//                 .secret("coucousecretlolilol".to_string())
//                 .build(),
//         )
//         .build();
//
//     let resp = helix_client
//         .req_post(
//             helix::eventsub::CreateEventSubSubscriptionRequest::builder().build(),
//             body,
//             token,
//         )
//         .await;
//
//     match resp {
//         Ok(_) => (),
//         Err(err) => match err {
//             // conflict: the subscription already exists, don't crash on that
//             ClientRequestError::HelixRequestPostError(HelixRequestPostError::Error {
//                 status,
//                 ..
//             }) if status == 409 => {
//                 println!("Subscription already exists, ignoring.");
//             }
//             _ => return Err(err.into()),
//         },
//     };
//
//     let body = helix::eventsub::CreateEventSubSubscriptionBody::builder()
//         .subscription(
//             eventsub::stream::StreamOfflineV1::builder()
//                 .broadcaster_user_id(user.id)
//                 .build(),
//         )
//         .transport(
//             eventsub::Transport::builder()
//                 .method(eventsub::TransportMethod::Webhook)
//                 .callback(cb_uri.clone())
//                 .secret("coucousecretlolilol".to_string())
//                 .build(),
//         )
//         .build();
//
//     let resp = helix_client
//         .req_post(
//             helix::eventsub::CreateEventSubSubscriptionRequest::builder().build(),
//             body,
//             token,
//         )
//         .await;
//
//     match resp {
//         Ok(_) => (),
//         Err(err) => match err {
//             // conflict: the subscription already exists, don't crash on that
//             ClientRequestError::HelixRequestPostError(HelixRequestPostError::Error {
//                 status,
//                 ..
//             }) if status == 409 => {
//                 println!("Subscription already exists, ignoring.");
//             }
//             _ => return Err(err.into()),
//         },
//     };
//
//     Ok(())
// }
