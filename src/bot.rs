use anyhow::Result;
use futures::prelude::*;
use irc::client::prelude::*;
use std::env;
use std::sync::{Arc, Mutex};

use crate::crypto;
use crate::ctcp;
use crate::joke;
use crate::parser;
use crate::republican_calendar;

pub async fn run_bot(client: &Arc<Mutex<Client>>) -> Result<()> {
    let blacklisted_users = vec!["coucoubot", "lambdacoucou", "M`arch`ov", "coucoucou"];
    {
        client.lock().unwrap().identify()?;
    }

    sasl_auth(&client)?;

    let mut stream = {
        let mut client = client.lock().unwrap();
        client.stream()?
    };

    while let Some(irc_message) = stream.next().await.transpose()? {
        let response_target = match irc_message.response_target() {
            Some(t) => t.to_string(),
            None => continue,
        };

        log::debug!("got a message: {:?}", irc_message);
        let source_nickname = irc_message
            .source_nickname()
            .map(|s| s.to_string())
            .unwrap_or("".to_string());

        if blacklisted_users.contains(&&source_nickname[..]) {
            log::debug!(
                "message from blacklisted user: {}, discarding",
                source_nickname
            );
            continue;
        }

        if let Command::PRIVMSG(_source, message) = irc_message.command {
            let parsed_command = parser::parse_command(&message);

            match parsed_command {
                Err(err) => {
                    log::error!("error parsing message: {} from: {}", err, message);
                    let msg = format!("error parsing message: {} from: {}", err, message);
                    client.lock().unwrap().send_privmsg("Geekingfrog", msg)?;
                }
                Ok(cmd) => match cmd {
                    parser::CoucouCmd::CTCP(ctcp) => {
                        ctcp::handle_ctcp(&client, response_target, ctcp)?;
                    }
                    parser::CoucouCmd::Date(mb_target) => {
                        match republican_calendar::handle_command(mb_target) {
                            None => (),
                            Some(msg) => {
                                client.lock().unwrap().send_privmsg(response_target, msg)?
                            }
                        }
                    }
                    parser::CoucouCmd::Joke(mb_target) => {
                        match joke::handle_command(mb_target).await {
                            None => (),
                            Some(msg) => {
                                client.lock().unwrap().send_privmsg(response_target, msg)?
                            }
                        }
                    }
                    parser::CoucouCmd::Crypto(coin, mb_target) => {
                        match crypto::handle_command(coin, mb_target).await {
                            None => (),
                            Some(msg) => {
                                client.lock().unwrap().send_privmsg(response_target, msg)?
                            }
                        }
                    }
                    parser::CoucouCmd::Other(_) => (),
                },
            }
        }
    }

    Ok(())
}

fn sasl_auth(client: &Arc<Mutex<Client>>) -> Result<()> {
    match env::var("SASL_PASSWORD") {
        Ok(password) => {
            log::info!("Authenticating with SASL");
            let client = client.lock().unwrap();
            client.send_cap_req(&[Capability::Sasl])?;
            client.send_sasl_plain()?;
            let nick = client.current_nickname();
            let sasl_str = base64::encode(format!("{}\0{}\0{}", nick, nick, password));
            client.send(Command::AUTHENTICATE(sasl_str))?;
            log::info!("SASL authenticated (hopefully)");
            Ok(())
        }
        Err(env::VarError::NotPresent) => {
            log::info!("No SASL_PASSWORD env var found, not authenticating anything.");
            Ok(())
        }
        Err(env::VarError::NotUnicode(os_str)) => Err(anyhow!(
            "SASL_PASSWORD not valid unicode string! {:?}",
            os_str
        )),
    }
}
