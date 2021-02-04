#[macro_use]
extern crate tokio;

use irc::client::prelude::*;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate diesel;

#[macro_use]
extern crate diesel_migrations;

use anyhow::{Result, Context};
use structopt::StructOpt;
use std::sync::{Arc, Mutex};

mod ctcp;
mod db;
mod joke;
mod parser;
mod republican_calendar;
mod utils;
mod crypto;
mod schema;
mod bot;

#[derive(Debug, StructOpt)]
struct Opt {
    /// list of channels to join
    #[structopt(long)]
    channels: Vec<String>,

    #[structopt(long, default_value = "rustycoucou")]
    nickname: String,

    #[structopt(long, default_value = "chat.freenode.net")]
    server: String,

    #[structopt(long, default_value = "7000")]
    port: u16,

    #[structopt(long)]
    disable_tls: bool,
}

// #[tokio::main]
// async fn main() -> Result<()> {
//     // tokio::try_join!(do_async1(), do_async2())?;
//     let (tag, res) = tokio::select!{
//         // a = do_async1() => a.and_then(|_| Err(anyhow!("finished early!"))).context("async1").unwrap_err(),
//         // a = do_async2() => a.and_then(|_| Err(anyhow!("finished early!"))).context("async2").unwrap_err(),
//         a = do_async1() => {("async1", a)}
//         a = do_async2() => {("async2", a)}
//     };
//     match res {
//         Ok(_) => println!("{} finished early", tag),
//         Err(err) => println!("{} errored with: {}", tag, err),
//     };
//     Ok(())
// }
//
// async fn do_async1() -> Result<()> {
//     for i in 0..=4 {
//         println!("coucou1 {}", i);
//         tokio::time::sleep(std::time::Duration::from_secs(1)).await;
//     }
//     Ok(())
// }
//
// async fn do_async2() -> Result<()> {
//     tokio::time::sleep(std::time::Duration::from_secs(3)).await;
//     println!("done async 2");
//     Err(anyhow!("oh noooes"))
// }

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // db::get_and_save_rates().await?;
    // crypto::test()?;
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
        monitor_crypto_coins(),
        run_bot(&client),
    )?;

    println!("done");
    Ok(())
}

// async closures are unstable, so create these function in order to
// add the anyhow::Context bit
async fn run_bot(client: &Arc<Mutex<Client>>) -> Result<()> {
    bot::run_bot(client).await.context("Bot crashed")
}

async fn monitor_crypto_coins() -> Result<()> {
    crypto::monitor_crypto_coins().await.context("Monitoring of crypto coins crashed")
}
