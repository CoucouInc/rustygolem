extern crate tokio;

use irc::client::prelude::*;
#[macro_use]
extern crate anyhow;

use anyhow::Result;
// use futures::stream;
use futures::prelude::*;
// use futures::prelude::stream::*;
// use itertools::Itertools;

mod ctcp;
mod parser;
mod republican_calendar;

#[tokio::main]
async fn main() -> Result<()> {

    let blacklisted_users = vec![
        "coucoubot",
        "lambdacoucou",
        "M`arch`ov",
        "coucoucou",
    ];

    let config = Config {
        owners: vec!["Geekingfrog".to_string()],
        nickname: Some("rustycoucou".to_string()),
        server: Some("chat.freenode.net".to_string()),
        // port: Some(6667),
        // use_tls: Some(false),
        port: Some(7000),
        use_tls: Some(true),
        channels: vec!["#gougoutest".to_string()],
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
                println!("message from blacklisted user: {}, discarding", source_nickname);
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
                    parser::CoucouCmd::Other(_) => (),
                },
            }
        }
    }

    println!("done");
    Ok(())
}
