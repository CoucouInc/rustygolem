use anyhow::{Context, Result};
use futures::prelude::*;
use irc::client::prelude::*;
use serde::Deserialize;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::crypto;
use crate::ctcp;
use crate::joke;
use crate::parser;
use crate::republican_calendar;
use crate::twitch;

#[derive(Debug)]
enum BotMessage {
    Irc(irc::client::prelude::Message),
    Twitch(twitch::message::Message),
}

#[derive(Debug, Deserialize)]
pub struct Config {
    twitch_module: twitch::config::Config,
}

#[derive(Debug)]
pub struct Bot {
    irc_client: Arc<Mutex<Client>>,
    blacklisted_users: Vec<&'static str>,
    config: Config,
}

impl Bot {
    pub fn new(irc_client: Client, blacklisted_users: Vec<&'static str>) -> Result<Self> {
        // TODO refactor that to create the client from config/args
        let config = serde_dhall::from_file("bot_config.dhall")
            .parse::<Config>()
            .with_context(|| "Failed to parse bot config")?;

        Ok(Self {
            irc_client: Arc::new(Mutex::new(irc_client)),
            blacklisted_users,
            config,
        })
    }

    pub async fn run(&self) -> Result<()> {
        {
            self.irc_client.lock().unwrap().identify()?;
        }

        // blocking but shrug
        sasl_auth(self.irc_client.clone())?;

        let (tx, rx) = mpsc::channel(100);
        tokio::try_join!(self.read_messages(tx), self.process_messages(rx))?;
        Ok(())
    }

    async fn read_messages(&self, tx: mpsc::Sender<BotMessage>) -> Result<()> {
        tokio::try_join!(
            self.read_irc_messages(tx.clone()),
            self.read_twitch_messages(tx)
        )?;
        Ok(())
    }

    async fn read_irc_messages(&self, tx: mpsc::Sender<BotMessage>) -> Result<()> {
        let mut stream = {
            let mut client = self.irc_client.lock().unwrap();
            client.stream()?
        };

        while let Some(irc_message) = stream.next().await.transpose()? {
            tx.send(BotMessage::Irc(irc_message)).await?
        }
        Ok(())
    }

    async fn read_twitch_messages(&self, tx: mpsc::Sender<BotMessage>) -> Result<()> {
        let (twitch_tx, mut twitch_rx) = mpsc::channel(50);
        let consume_msg = async move {
            while let Some(twitch_msg) = twitch_rx.recv().await {
                tx.send(BotMessage::Twitch(twitch_msg)).await?
            }
            Ok(())
        };
        tokio::try_join!(twitch::webhook_server::run_server(twitch_tx), consume_msg)?;
        Ok(())
    }

    async fn process_messages(&self, mut rx: mpsc::Receiver<BotMessage>) -> Result<()> {
        while let Some(msg) = rx.recv().await {
            match msg {
                BotMessage::Irc(msg) => self.process_irc_message(msg).await?,
                BotMessage::Twitch(msg) => self.process_twitch_message(msg).await?,
            }
        }
        Ok(())
    }

    async fn process_irc_message(&self, irc_message: Message) -> Result<()> {
        let response_target = match irc_message.response_target() {
            Some(t) => t.to_string(),
            None => return Ok(()),
        };

        log::debug!("got a message: {:?}", irc_message);
        let source_nickname = irc_message
            .source_nickname()
            .map(|s| s.to_string())
            .unwrap_or("".to_string());

        if self.blacklisted_users.contains(&&source_nickname[..]) {
            log::debug!(
                "message from blacklisted user: {}, discarding",
                source_nickname
            );
            return Ok(());
        }

        if let Command::PRIVMSG(_source, message) = irc_message.command {
            let parsed_command = parser::parse_command(&message);

            match parsed_command {
                Err(err) => {
                    log::error!("error parsing message: {} from: {}", err, message);
                    let msg = format!("error parsing message: {} from: {}", err, message);
                    self.irc_client
                        .lock()
                        .unwrap()
                        .send_privmsg("Geekingfrog", msg)?;
                }
                Ok(cmd) => match cmd {
                    parser::CoucouCmd::CTCP(ctcp) => {
                        ctcp::handle_ctcp(&self.irc_client, response_target, ctcp)?;
                    }
                    parser::CoucouCmd::Date(mb_target) => {
                        match republican_calendar::handle_command(mb_target) {
                            None => (),
                            Some(msg) => self
                                .irc_client
                                .lock()
                                .unwrap()
                                .send_privmsg(response_target, msg)?,
                        }
                    }
                    parser::CoucouCmd::Joke(mb_target) => {
                        match joke::handle_command(mb_target).await {
                            None => (),
                            Some(msg) => self
                                .irc_client
                                .lock()
                                .unwrap()
                                .send_privmsg(response_target, msg)?,
                        }
                    }
                    parser::CoucouCmd::Crypto(coin, mb_target) => {
                        match crypto::handle_command(coin, mb_target).await {
                            None => (),
                            Some(msg) => self
                                .irc_client
                                .lock()
                                .unwrap()
                                .send_privmsg(response_target, msg)?,
                        }
                    }
                    parser::CoucouCmd::Other(_) => (),
                },
            }
        }

        Ok(())
    }

    async fn process_twitch_message(&self, msg: twitch::message::Message) -> Result<()> {
        log::info!("Got a twitch message! {:?}", msg);
        match msg {
            twitch::message::Message::StreamOnline(online) => {
                let target = self
                    .config
                    .twitch_module
                    .watched_streams
                    .iter()
                    .find(|s| s.nickname == online.broadcaster_user_login);
                match target {
                    None => log::warn!(
                        "Got a notification for {} but not found in config",
                        online.broadcaster_user_login
                    ),
                    Some(target) => {
                        // TODO sending message is sync, make that async
                        let client = self.irc_client.lock().expect("irc lock");
                        let message =
                            format!("Le stream de {} est maintenant live !", target.nickname);
                        for chan in &target.irc_channels {
                            client.send_privmsg(chan, &message)?;
                        }
                    }
                };
                log::info!("target found: {:?}", target);
            }

            twitch::message::Message::StreamOffline(offline) => {
                let target = self
                    .config
                    .twitch_module
                    .watched_streams
                    .iter()
                    .find(|s| s.nickname == offline.broadcaster_user_login);
                log::info!("target found: {:?}", target);
                match target {
                    None => log::warn!(
                        "Got a notification for {} but not found in config",
                        offline.broadcaster_user_login
                    ),
                    Some(target) => {
                        // TODO sending message is sync, make that async
                        let client = self.irc_client.lock().expect("irc lock");
                        let message =
                            format!("{} a arreté de streamer pour le moment. N'oubliez pas de like&subscribe.", target.nickname);
                        for chan in &target.irc_channels {
                            client.send_privmsg(chan, &message)?;
                        }
                    }
                };
            }
        }
        Ok(())
    }
}

fn sasl_auth(client: Arc<Mutex<Client>>) -> Result<()> {
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
