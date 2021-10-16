use anyhow::{Context, Result};
use futures::prelude::*;
use irc::client::prelude::*;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::config::BotConfig;
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

#[derive(Debug, Default)]
struct State {
    // the index in config.twitch_module.config
    twitch_module: twitch::state::State,
}

#[derive(Debug)]
pub struct Bot {
    irc_client: Arc<Mutex<Client>>,
    config: BotConfig,
    state: State,
    twitch_client: Option<twitch::client::Client>,
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
        let config =
            BotConfig::from_path(config_path).with_context(|| "Failed to parse bot config")?;

        let twitch_client = if config.twitch_module.is_enabled {
            let conf = config.twitch_module.clone();
            Some(twitch::client::Client::new_from_config(conf).await?)
        } else {
            log::info!("Twitch module disabled");
            None
        };

        Ok(Self {
            irc_client: Arc::new(Mutex::new(irc_client)),
            config,
            state: Default::default(),
            twitch_client,
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
        let consume_msg = || async move {
            while let Some(twitch_msg) = twitch_rx.recv().await {
                tx.send(BotMessage::Twitch(twitch_msg)).await?
            }
            Ok::<(), anyhow::Error>(())
        };

        // let tasks: Vec<Box<dyn TryFuture + Unpin>> = vec![consume_msg()];
        let mut tasks: Vec<Box<dyn Future<Output = Result<()>> + Unpin>> =
            vec![Box::new(Box::pin(consume_msg()))];

        if self.config.twitch_module.is_enabled {
            log::info!("Enabling twitch module");
            tasks.push(Box::new(Box::pin(twitch::client::ensure_subscriptions(
                self.config.twitch_module.clone(),
            ))));
            tasks.push(Box::new(Box::pin(twitch::webhook_server::run_server(
                &self.config.twitch_module,
                twitch_tx,
            ))));
        }

        futures::future::try_join_all(tasks).await?;
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
                    .find(|s| s.nickname == online.broadcaster_user_login);
                match target {
                    None => log::warn!(
                        "Got a notification for {} but not found in config",
                        online.broadcaster_user_login
                    ),
                    Some(target) => {
                        let nick = target.nickname.clone();
                        let stream = self
                            .twitch_client
                            .as_ref()
                            .unwrap()
                            .get_live_stream(nick.clone())
                            .await?;

                        match stream {
                            None => log::info!("Got stream live notification but twitch returned nothing. TOCTOU :shrug:"),
                            Some(stream) => {

                                let url = format!("https://www.twitch.tv/{}", &target.nickname);
                                let message =
                                    format!("Le stream de {} est maintenant live at {} ({})!",
                                    target.nickname,
                                    url, stream.game_name
                                );
                                self.state.twitch_module.add_stream(nick, stream);
                                // TODO sending message is sync, make that async
                                let client = self.irc_client.lock().expect("irc lock");
                                for chan in &target.irc_channels {
                                    client.send_privmsg(chan, &message)?;
                                }
                            },
                        }
                    }
                };
            }

            twitch::message::Message::StreamOffline(offline) => {
                let target = self
                    .config
                    .twitch_module
                    .watched_streams
                    .iter()
                    .find(|s| s.nickname == offline.broadcaster_user_login);
                match target {
                    None => log::warn!(
                        "Got a notification for {} but not found in config",
                        offline.broadcaster_user_login
                    ),
                    Some(target) => {
                        // TODO sending message is sync, make that async
                        match self.state.twitch_module.remove_stream(&target.nickname) {
                            None => {
                                // this can happen when a streams goes online/offline rapidly,
                                // twitch only sends the offline event.
                                log::warn!(
                                    "Got an offline notification for a stream not marked live"
                                );
                            }
                            Some(_s) => {
                                let client = self.irc_client.lock().expect("irc lock");
                                let message =
                                    format!("{} a arreté de streamer pour le moment. N'oubliez pas de like&subscribe.", target.nickname);
                                for chan in &target.irc_channels {
                                    client.send_privmsg(chan, &message)?;
                                }
                            }
                        }
                    }
                };
            }
        }
        Ok(())
    }
}
