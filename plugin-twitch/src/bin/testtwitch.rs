#![allow(dead_code, unused_imports)]

use anyhow::{Context, Result};
use twitch_api2::{
    helix::{
        streams,
        users::{self, get_users},
    },
    twitch_oauth2::AppAccessToken,
    HelixClient, types::UserId,
};

#[tokio::main]
async fn main() -> Result<()> {
    let client: HelixClient<'static, reqwest::Client> = twitch_api2::HelixClient::default();

    let auth_client = reqwest::Client::default();
    let token = AppAccessToken::get_app_access_token(
        &auth_client,
        std::env::var("TWITCH_CLIENT_ID")
            .expect("twitch client id")
            .into(),
        std::env::var("TWITCH_CLIENT_SECRET")
            .expect("twitch client secret")
            .into(),
        vec![], // scopes
    )
    .await
    .context("Cannot get app access token")?;

    // broadcaster_user_id: "42481408", broadcaster_user_login: "juantitor", broadcaster_user_name: "JuanTitor", id: "44862854828", type_: Live

    let resp = client
        .req_get(
            streams::GetStreamsRequest::builder()
                .user_login(vec![
                    "juantitor".into(),
                    "JuanTitor".into(),
                ])
                .user_id(vec!["42481408".into()])
                // .user_id(vec!["44862854828".into()])
                .build(),
            &token,
        )
        .await?;

    dbg!(resp);

    // let resp = client
    //     .req_get(
    //         get_users::GetUsersRequest::builder()
    //             .login(vec!["juantitor".into()])
    //             .build(),
    //         &token,
    //     )
    //     .await?;
    //
    // dbg!(resp);

    Ok(())
}
