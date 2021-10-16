use anyhow::{Context, Result};
use twitch::client::Client as TwitchClient;

mod config;
mod twitch;

// use crate::twitch::config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let config = config::BotConfig::from_path("bot_config.dhall")
        .with_context(|| "Failed to parse bot config")?;

    let tm = TwitchClient::new_from_config(config.twitch_module).await?;

    // let subs = tm.list_subscriptions().await?;
    // for sub in &subs {
    //     println!("{:?}", sub);
    // }

    // let users = tm
    //     .get_users(vec![], subs.iter().map(|s| s.user_id.clone()).collect())
    //     .await?;
    // for user in &users {
    //     println!("--------------------------------------------------");
    //     println!("{:#?}", user);
    // }

    let resp = tm
        .client
        .get_channel_from_login("artart78".to_string(), &tm.token)
        .await?;
    println!("{:?}", resp);

    let resp = tm.get_live_streams().await?;

    println!("{:?}", resp);

    Ok(())
}
