use crate::plugin::{self, Plugin};
use crate::plugins;
use anyhow::{Context, Result};
use futures::prelude::*;
use irc::proto::Message;
use serde::Deserialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Deserialize)]
struct GolemConfig {
    blacklisted_users: Vec<String>,
    plugins: Vec<String>,
    sasl_password: Option<String>,
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
    sasl_password: Option<String>,
    blacklisted_users: Vec<String>,
    plugins: Vec<Box<dyn Plugin>>,
}

impl Golem {
    #[allow(dead_code)]
    pub async fn new_from_config(irc_config: irc::client::data::Config) -> Result<Self> {
        let irc_client = irc::client::Client::from_config(irc_config).await?;
        let conf = GolemConfig::from_path("golem_config.dhall")?;
        let plugins = stream::iter(conf.plugins)
            .map(|name| async move { init_plugin(&name).await })
            .buffer_unordered(10)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            irc_client: Arc::new(Mutex::new(irc_client)),
            sasl_password: conf.sasl_password,
            blacklisted_users: conf.blacklisted_users,
            plugins,
        })
    }

    pub async fn run(&self) -> Result<()> {
        // blocking but shrug
        self.authenticate()
            .context("Problem while authenticating")?;

        tokio::try_join!(self.run_plugins(), self.recv_irc_messages(),)?;

        log::error!("golem exited");
        Ok(())
    }

    fn authenticate(&self) -> Result<()> {
        match self.sasl_password {
            None => {
                log::info!("No SASL_PASSWORD env var found, not authenticating anything.");
                self.irc_client.lock().unwrap().identify()?;
                Ok(())
            }
            Some(ref password) => {
                log::info!("Authenticating with SASL");
                let client = self.irc_client.lock().unwrap();
                client.send_cap_req(&[irc::proto::Capability::Sasl])?;
                client.send_sasl_plain()?;
                let nick = client.current_nickname();
                let sasl_str = base64::encode(format!("{}\0{}\0{}", nick, nick, password));
                client.send(irc::proto::Command::AUTHENTICATE(sasl_str))?;
                client.identify()?;
                log::info!("SASL authenticated (hopefully)");
                Ok(())
            }
        }
    }

    async fn recv_irc_messages(&self) -> Result<()> {
        let mut stream = {
            let mut client = self.irc_client.lock().unwrap();
            client.stream()?
        };

        while let Some(irc_message) = stream.next().await.transpose()? {
            if let Some(source) = irc_message.source_nickname() {
                if self.blacklisted_users.contains(&source.to_string()) {
                    log::debug!("message from blacklisted user: {}, discarding", source);
                    continue;
                }
            }

            let messages = self.plugins_in_messages(&irc_message)
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
    ) -> Result<Vec<Option<Message>>> {
        let mut results = Vec::with_capacity(self.plugins.len());

        let (txs, rxs): (Vec<_>, Vec<_>) = self.plugins.iter().map(|_| oneshot::channel()).unzip();

        futures::stream::iter(self.plugins.iter().zip(txs))
            .map(Ok)
            .try_for_each_concurrent(5, |(plugin, tx)| async move {
                let mb_msg = plugin.in_message(msg).await.with_context(|| {
                    format!("in_message error from plugin {}", plugin.get_name())
                })?;
                if tx.send(mb_msg).is_err() {
                    return Err(anyhow!("cannot send plugin message !"));
                }
                Ok::<(), anyhow::Error>(())
            })
            .await?;

        for rx in rxs {
            let rx: oneshot::Receiver<Option<Message>> = rx;
            results.push(rx.await?);
        }

        Ok(results)
    }

    async fn run_plugins(&self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(10);
        let runs = self.plugins.iter().map(|p| {
            let tx = tx.clone();
            async move {
                p.run(tx)
                    .await
                    .with_context(|| format!("Plugin {}.run() failed", p.get_name()))?;
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

    async fn outbound_message(&self, message: &Message) -> Result<()> {
        // TODO don't crash if a plugin returns an error
        futures::stream::iter(self.plugins.iter())
            .map(Ok)
            .try_for_each_concurrent(5, |plugin| {
                let msg = &message;
                async move {
                    plugin.out_message(msg).await?;
                    Ok::<(), anyhow::Error>(())
                }
            })
            .await?;
        let client = self.irc_client.lock().expect("lock golem irc client");
        client.send(message.clone())?;
        Ok(())
    }
}

async fn init_plugin(name: &str) -> Result<Box<dyn Plugin>> {
    // TODO: generate a macro which automatically match the name
    // with the correct module based on the exports of crate::plugins
    let plugin = match name {
        "crypto" => plugin::new_boxed::<plugins::Crypto>().await,
        "ctcp" => plugin::new_boxed::<plugins::Ctcp>().await,
        "echo" => plugin::new_boxed::<plugins::Echo>().await,
        "joke" => plugin::new_boxed::<plugins::Joke>().await,
        "republican_calendar" => plugin::new_boxed::<plugins::RepublicanCalendar>().await,
        "twitch" => plugin::new_boxed::<plugins::Twitch>().await,
        _ => return Err(anyhow!("Unknown plugin name: {}", name)),
    };
    let plugin = plugin.with_context(|| format!("Cannot initalize plugin {}", name))?;
    log::info!("Plugin initialized: {}", name);
    Ok(plugin)
}
