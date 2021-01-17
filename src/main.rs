extern crate tokio;

use irc::client::prelude::*;
#[macro_use]
extern crate anyhow;

use anyhow::Result;
use futures::prelude::*;
use structopt::StructOpt;

mod ctcp;
mod joke;
mod parser;
mod republican_calendar;
mod utils;

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

#[tokio::main]
async fn main() -> Result<()> {
    let blacklisted_users = vec!["coucoubot", "lambdacoucou", "M`arch`ov", "coucoucou"];

    let opt = Opt::from_args();

    if opt.channels.is_empty() {
        return Err(anyhow!("No channels to join, aborting"));
    }

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

    let mut client = Client::from_config(config).await?;
    client.identify()?;
    let mut stream = client.stream()?;

    while let Some(irc_message) = stream.next().await.transpose()? {
        let response_target = match irc_message.response_target() {
            Some(t) => t.to_string(),
            None => continue,
        };

        // println!("got a message: {:#?}", irc_message);
        let source_nickname = irc_message
            .source_nickname()
            .map(|s| s.to_string())
            .unwrap_or("".to_string());

        if let Command::PRIVMSG(_source, message) = irc_message.command {
            if blacklisted_users.contains(&&source_nickname[..]) {
                println!(
                    "message from blacklisted user: {}, discarding",
                    source_nickname
                );
                continue;
            }

            let parsed_command = parser::parse_command(&message);

            match parsed_command {
                Err(err) => eprintln!("error parsing message: {} from: {}", err, message),
                Ok(cmd) => match cmd {
                    parser::CoucouCmd::CTCP(ctcp) => {
                        ctcp::handle_ctcp(&client, response_target, ctcp)?;
                    }
                    parser::CoucouCmd::Date(mb_target) => {
                        match republican_calendar::handle_command(mb_target) {
                            None => (),
                            Some(msg) => client.send_privmsg(response_target, msg)?,
                        }
                    }
                    parser::CoucouCmd::Joke(mb_target) => {
                        match joke::handle_command(mb_target).await {
                            None => (),
                            Some(msg) => client.send_privmsg(response_target, msg)?,
                        }
                    }
                    parser::CoucouCmd::Other(_) => (),
                },
            }
        }
    }

    println!("done");
    Ok(())
}
