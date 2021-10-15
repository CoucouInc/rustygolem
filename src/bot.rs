use anyhow::{Context, Result};
use futures::prelude::*;
use irc::client::prelude::*;
use serde::Deserialize;
use std::path::Path;
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
    blacklisted_users: Vec<String>,
    sasl_password: Option<String>,
    twitch_module: twitch::config::Config,
}

#[derive(Debug, Default)]
struct State {
    // the index in config.twitch_module.config
    twitch_module: twitch::state::State,
}

#[derive(Debug)]
pub struct Bot {
    irc_client: Arc<Mutex<Client>>,
    config: Config,
    state: State,
}

impl Bot {
    pub async fn new_from_config<P>(
        irc_config: irc::client::data::Config,
        config_path: P,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let irc_client = Client::from_config(irc_config).await?;
        let config = serde_dhall::from_file(config_path)
            .parse::<Config>()
            .with_context(|| "Failed to parse bot config")?;

        Ok(Self {
            irc_client: Arc::new(Mutex::new(irc_client)),
            config,
            state: Default::default(),
        })
    }

    pub async fn run(&self) -> Result<()> {
        // blocking but shrug
        self.authenticate()?;
        log::info!("Bot identified.");

        let (tx, rx) = mpsc::channel(100);
        tokio::try_join!(self.read_messages(tx), self.process_messages(rx))?;
        Ok(())
    }

    fn authenticate(&self) -> Result<()> {
        match self.config.sasl_password {
            None => {
                log::info!("No SASL_PASSWORD env var found, not authenticating anything.");
                self.irc_client.lock().unwrap().identify()?;
                Ok(())
            }
            Some(ref password) => {
                log::info!("Authenticating with SASL");
                let client = self.irc_client.lock().unwrap();
                client.send_cap_req(&[Capability::Sasl])?;
                client.send_sasl_plain()?;
                let nick = client.current_nickname();
                let sasl_str = base64::encode(format!("{}\0{}\0{}", nick, nick, password));
                client.send(Command::AUTHENTICATE(sasl_str))?;
                client.identify()?;
                log::info!("SASL authenticated (hopefully)");
                Ok(())
            }
        }
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

        tokio::try_join!(
            twitch::subscriptions::ensure_subscriptions(&self.config.twitch_module),
            twitch::webhook_server::run_server(&self.config.twitch_module, twitch_tx),
            consume_msg
        )?;
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

        if self.config.blacklisted_users.contains(&source_nickname) {
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
        log::debug!("Got a twitch message! {:?}", msg);
        match msg {
            twitch::message::Message::StreamOnline(online) => {
                let target = self
                    .config
                    .twitch_module
                    .watched_streams
                    .iter()
                    .enumerate()
                    .find(|(_, s)| s.nickname == online.broadcaster_user_login);
                match target {
                    None => log::warn!(
                        "Got a notification for {} but not found in config",
                        online.broadcaster_user_login
                    ),
                    Some((idx, target)) => {
                        // TODO sending message is sync, make that async
                        let client = self.irc_client.lock().expect("irc lock");
                        self.state.twitch_module.add_stream(idx);

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
                    .enumerate()
                    .find(|(_, s)| s.nickname == offline.broadcaster_user_login);
                log::info!("target found: {:?}", target);
                match target {
                    None => log::warn!(
                        "Got a notification for {} but not found in config",
                        offline.broadcaster_user_login
                    ),
                    Some((idx, target)) => {
                        // TODO sending message is sync, make that async
                        if self.state.twitch_module.remove_stream(idx) {
                            let client = self.irc_client.lock().expect("irc lock");
                            let message =
                                format!("{} a arreté de streamer pour le moment. N'oubliez pas de like&subscribe.", target.nickname);
                            for chan in &target.irc_channels {
                                client.send_privmsg(chan, &message)?;
                            }
                        } else {
                            // this can happen when a streams goes online/offline rapidly,
                            // twitch only sends the offline event.
                            log::warn!("Got an offline notification for a stream not marked live");
                        }
                    }
                };
            }
        }
        Ok(())
    }
}
