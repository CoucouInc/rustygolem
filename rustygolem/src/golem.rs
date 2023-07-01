use crate::plugins;
use anyhow::{Context, Result};
use axum::Router;
use futures::prelude::*;
use irc::client::ClientStream;
use irc::proto::{CapSubCommand, Command, Message, Response};
use plugin_core::{Initialised, Plugin};
use serde::Deserialize;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};
use tokio::time::timeout;

#[derive(Debug, Deserialize)]
struct GolemConfig {
    blacklisted_users: Vec<String>,
    plugins: Vec<String>,
    sasl_password: Option<String>,
    server_bind_address: String,
    server_bind_port: u16,
}

impl GolemConfig {
    pub fn from_path<P>(config_path: P) -> std::result::Result<GolemConfig, serde_dhall::Error>
    where
        P: AsRef<Path>,
    {
        serde_dhall::from_file(config_path).parse::<GolemConfig>()
    }
}

pub struct Golem {
    irc_client: Arc<Mutex<irc::client::Client>>,
    message_stream: AsyncMutex<ClientStream>,
    sasl_password: Option<String>,
    blacklisted_users: Vec<String>,
    plugins: Vec<Box<dyn Plugin>>,
    /// bind the local server on this address
    address: std::net::SocketAddr,
    /// axum router so that plugins can define their own routes and state
    /// if required. For example for webhooks
    router: Option<Router<()>>,
}

impl Golem {
    #[allow(dead_code)]
    pub async fn new_from_config(
        irc_config: irc::client::data::Config,
        golem_config_path: String,
    ) -> Result<Self> {
        let mut irc_client = irc::client::Client::from_config(irc_config).await?;
        let conf = GolemConfig::from_path(&golem_config_path)
            .with_context(|| format!("Cannot parse golem config at {golem_config_path}"))?;
        log::debug!("Loaded config: {conf:?}");

        let core_config = plugin_core::Config {
            config_path: golem_config_path,
        };
        let core_config = Arc::new(core_config);

        let inits = stream::iter(conf.plugins)
            .map(|name| {
                let core_config = Arc::clone(&core_config);
                async move { init_plugin(&core_config, &name).await }
            })
            .buffer_unordered(10)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        let mut router: Option<Router<()>> = None;
        let mut plugins = Vec::with_capacity(inits.len());
        for init in inits {
            if let Some(r) = init.router {
                match router {
                    Some(x) => {
                        log::info!("Mounting a router from plugin {}", init.plugin.get_name());
                        router = Some(x.merge(r))
                    }
                    None => router = Some(r),
                }
            }
            plugins.push(init.plugin);
        }

        let addr = std::net::IpAddr::from_str(&conf.server_bind_address)?;
        let address = std::net::SocketAddr::from((addr, conf.server_bind_port));
        let message_stream = irc_client.stream()?;

        Ok(Self {
            irc_client: Arc::new(Mutex::new(irc_client)),
            message_stream: AsyncMutex::new(message_stream),
            sasl_password: conf.sasl_password,
            blacklisted_users: conf.blacklisted_users,
            plugins,
            address,
            router,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.authenticate_and_identify()
            .await
            .context("Problem while authenticating")?;

        let router = self.router.take();

        tokio::try_join!(
            self.run_plugins(),
            self.recv_irc_messages(),
            self.run_server(router)
        )?;

        log::error!("golem exited");
        Ok(())
    }

    async fn authenticate_and_identify(&self) -> Result<()> {
        match self.sasl_password {
            None => {
                log::info!("No SASL_PASSWORD env var found, not authenticating anything.");
                self.irc_client.lock().unwrap().identify()?;
                Ok(())
            }
            Some(ref password) => {
                self.sasl_auth(password).await?;
                Ok(())
            }
        }
    }

    // SASL PLAIN authentication
    // https://ircv3.net/specs/extensions/sasl-3.1.html
    async fn sasl_auth(&self, password: &str) -> Result<()> {
        let client = self.irc_client.lock().unwrap();
        let nick = client.current_nickname();
        log::info!("Authenticating with SASL for {nick}");

        client.send_cap_req(&[irc::proto::Capability::Sasl])?;
        // the call client.identify() provided by the irc library starts
        // by sending a CAP END before sending NICK and USER messages.
        // but as far as I can tell, this is incorrect for SASL, so manually send
        // the stuff
        client.send(Command::NICK(nick.to_string()))?;
        client.send(Command::USER(
            nick.to_string(),
            "0".to_string(),
            format!(":{nick}"),
        ))?;

        let duration = Duration::from_secs(10);
        timeout(
            duration,
            self.wait_for_message(|msg| match &msg.command {
                Command::CAP(_, CapSubCommand::ACK, Some(opt), _) if opt == "sasl" => true,
                _ => false,
            }),
        )
        .await
        .context("Timeout waiting for CAP ACK sasl")??;

        log::info!("GOT ACK for SASL !");
        client.send_sasl_plain()?;

        timeout(
            duration,
            self.wait_for_message(|msg| match &msg.command {
                Command::AUTHENTICATE(s) if s == "+" => true,
                _ => false,
            }),
        )
        .await
        .context("Timeout waiting for AUTHENTICATE + from server")??;

        let sasl_str = base64::encode(format!("\0{}\0{}", nick, password));
        client.send(Command::AUTHENTICATE(sasl_str))?;

        let resp = timeout(
            duration,
            self.wait_for_message(|msg| match &msg.command {
                Command::Response(Response::RPL_SASLSUCCESS, _) => true,
                Command::Response(resp, _) if is_sasl_error(resp) => true,
                _ => false,
            }),
        )
        .await
        .context("Timeout waiting for SASL acknowledment")??;

        if matches!(resp.command, Command::Response(resp, _) if is_sasl_error(&resp)) {
            anyhow::bail!("SASL auth failed {resp:?}");
        }
        log::info!("SASL authenticated");

        client.send(Command::CAP(None, CapSubCommand::END, None, None))?;
        log::info!("Handshake finished, ready to work");

        Ok(())
    }

    /// wait until the client receive a message that matches the given predicate
    /// and returns it. Warning, use timeout to prevent a deadlock.
    async fn wait_for_message<F>(&self, pred: F) -> Result<Message>
    where
        F: Fn(&Message) -> bool,
    {
        let mut message_stream = self.message_stream.lock().await;
        while let Some(message) = message_stream.next().await.transpose()? {
            if pred(&message) {
                return Ok(message);
            }
        }
        anyhow::bail!("Waited for message failed");
    }

    async fn recv_irc_messages(&self) -> Result<()> {
        let mut message_stream = self.message_stream.lock().await;
        while let Some(irc_message) = message_stream.next().await.transpose()? {
            let messages = self
                .plugins_in_messages(&irc_message)
                .await
                .with_context(|| "Plugin error !")?;

            for message in messages.into_iter().flatten() {
                self.outbound_message(&message).await?;
            }
        }
        Err(anyhow!("IRC receiving stream exited"))
    }

    async fn plugins_in_messages(
        &self,
        msg: &Message,
    ) -> Result<Vec<Option<(&'static str, Message)>>> {
        let mut results = Vec::with_capacity(self.plugins.len());

        let (txs, rxs): (Vec<_>, Vec<_>) = self.plugins.iter().map(|_| oneshot::channel()).unzip();

        futures::stream::iter(self.plugins.iter().zip(txs))
            .map(Ok)
            .try_for_each_concurrent(5, |(plugin, tx)| async move {
                if let Some(source) = msg.source_nickname() {
                    if plugin.ignore_blacklisted_users()
                        && self.blacklisted_users.contains(&source.to_string())
                    {
                        log::debug!("Message from blacklisted user: {}, discarding", source);
                        if tx.send(None).is_err() {
                            return Err(anyhow!("cannot send plugin message !"));
                        };
                        return Ok::<(), anyhow::Error>(());
                    }
                }

                let mb_msg = plugin.in_message(msg).await.with_context(|| {
                    format!("in_message error from plugin {}", plugin.get_name())
                })?;
                let msg = mb_msg.map(|m| (plugin.get_name(), m));
                if tx.send(msg).is_err() {
                    return Err(anyhow!("cannot send plugin message !"));
                }
                Ok::<(), anyhow::Error>(())
            })
            .await?;

        for rx in rxs {
            let rx: oneshot::Receiver<Option<(&'static str, Message)>> = rx;
            results.push(rx.await?);
        }

        Ok(results)
    }

    async fn run_plugins(&self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(10);
        let runs = self.plugins.iter().map(|p| {
            let tx = tx.clone();
            // The logic here is a bit meh.
            // need to create an intermediate channel to add the plugin name
            // to the message. Would be nice to be able to map over a channel
            async move {
                let name = p.get_name();
                let (plug_tx, mut plug_rx) = mpsc::channel(1);
                futures::future::try_join(
                    async {
                        p.run(plug_tx)
                            .await
                            .with_context(|| format!("Plugin {}.run() failed", p.get_name()))?;
                        Ok::<(), anyhow::Error>(())
                    },
                    async {
                        while let Some(plugin_message) = plug_rx.recv().await {
                            tx.send((name, plugin_message))
                                .await
                                .with_context(|| format!("Plugin {}.run() failed", p.get_name()))?;
                        }
                        Ok::<(), anyhow::Error>(())
                    },
                )
                .await?;
                Ok::<(), anyhow::Error>(())
            }
        });
        let process = async move {
            while let Some(msg) = rx.recv().await {
                self.outbound_message(&msg).await?;
            }
            Ok::<(), anyhow::Error>(())
        };
        futures::future::try_join(futures::future::try_join_all(runs), process).await?;
        Ok(())
    }

    async fn outbound_message(&self, message: &(&'static str, Message)) -> Result<()> {
        // TODO don't crash if a plugin returns an error
        futures::stream::iter(self.plugins.iter())
            .map(Ok)
            .try_for_each_concurrent(5, |plugin| {
                let (orig_name, msg) = &message;
                async move {
                    if &plugin.get_name() != orig_name {
                        plugin.out_message(msg).await?;
                    }
                    Ok::<(), anyhow::Error>(())
                }
            })
            .await?;
        let client = self.irc_client.lock().expect("lock golem irc client");
        // TODO this is blocking
        client.send(message.1.clone())?;
        Ok(())
    }

    async fn run_server(&self, router: Option<Router<()>>) -> Result<()> {
        let router = match router {
            Some(r) => r,
            None => return Ok(()),
        };

        log::info!("Starting web server, listening on {}", self.address);
        axum::Server::bind(&self.address)
            .serve(router.into_make_service())
            .await?;
        Ok(())
    }
}

// The function https://docs.rs/irc/latest/irc/client/prelude/enum.Response.html#method.is_error
// is broken, and consider anything with a code above 400 to be an error
// which doesn't account for SASL successes 900, 901, 902 and 903
fn is_sasl_error(resp: &Response) -> bool {
    // https://ircv3.net/specs/extensions/sasl-3.1.html
    *resp as u16 >= 904
}

async fn init_plugin(config: &plugin_core::Config, name: &str) -> Result<Initialised> {
    // TODO: generate a macro which automatically match the name
    // with the correct module based on the exports of crate::plugins
    let plugin = match name {
        "crypto" => plugins::Crypto::init(&config).await,
        "ctcp" => plugins::Ctcp::init(&config).await,
        "echo" => plugins::Echo::init(&config).await,
        "joke" => plugins::Joke::init(&config).await,
        "republican_calendar" => plugins::RepublicanCalendar::init(&config).await,
        "twitch" => plugin_twitch::Twitch::init(&config).await,
        "url" => plugin_url::UrlPlugin::init(&config).await,
        _ => return Err(anyhow!("Unknown plugin name: {}", name)),
    };
    let plugin = plugin.with_context(|| format!("Cannot initalize plugin {}", name))?;
    log::info!("Plugin initialized: {}", name);
    Ok(plugin)
}
