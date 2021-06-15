#[macro_use]
extern crate tokio;
extern crate log;

use irc::client::prelude::*;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate diesel;

#[macro_use]
extern crate diesel_migrations;

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use structopt::StructOpt;

mod bot;
mod crypto;
mod ctcp;
mod db;
mod joke;
mod parser;
mod republican_calendar;
mod schema;
mod utils;
mod urbain;

#[derive(Debug, StructOpt)]
struct Opt {
    /// list of channels to join
    #[structopt(long)]
    channels: Vec<String>,

    #[structopt(long, default_value = "rustycoucou")]
    nickname: String,

    #[structopt(long, default_value = "irc.libera.chat")]
    server: String,

    #[structopt(long, default_value = "6668")]
    port: u16,

    #[structopt(long)]
    disable_tls: bool,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    env_logger::init();

    // println!("{:?}", crypto::handle_command(Ok(crypto::CryptoCoin::Bitcoin), Some("charlie")).await);
    // return Ok(());

    let opt = Opt::from_args();

    if opt.channels.is_empty() {
        return Err(anyhow!("No channels to join, aborting"));
    }

    let _db_conn: Result<_> = tokio::task::spawn_blocking(|| {
        let conn = db::establish_connection()?;
        db::run_migrations(&conn)?;
        Ok(conn)
    })
    .await?;

    println!("Joining channel(s): {:?}", opt.channels);

    let config = Config {
        owners: vec!["Geekingfrog".to_string()],
        nickname: Some(opt.nickname),
        server: Some(opt.server),
        port: Some(opt.port),
        use_tls: Some(!opt.disable_tls),
        channels: opt.channels,
        ..Config::default()
    };

    let client = Client::from_config(config).await?;
    client.identify()?;
    let client = Arc::new(Mutex::new(client));

    try_join!(
        async move {
            crypto::monitor_crypto_coins()
                .await
                .context("Monitoring of crypto coins crashed")
        },
        async move { bot::run_bot(&client).await.context("Bot crashed") }
    )?;

    println!("done");
    Ok(())
}
