use crate::plugin::Plugin;
use crate::plugins;
use anyhow::{Context, Result};
use futures::prelude::*;
use irc::proto::Message;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

pub struct Golem {
    irc_client: Arc<Mutex<irc::client::Client>>,
}

impl Golem {
    pub async fn new_from_config(irc_config: irc::client::data::Config) -> Result<Self> {
        let irc_client = irc::client::Client::from_config(irc_config).await?;
        Ok(Self {
            irc_client: Arc::new(Mutex::new(irc_client)),
        })
    }

    pub async fn run(&self) -> Result<()> {
        // blocking but shrug
        self.irc_client.lock().unwrap().identify()?;
        log::info!("authed");

        // let ps = vec![Box::new(crate::plugin::new::<plugins::Echo>().await?) as Box<dyn Plugin>];
        let ps: Vec<Box<dyn Plugin>> = Vec::new();
        let (tx, rx) = mpsc::channel(10);
        tokio::try_join!(
            self.recv_irc_messages(tx.clone()),
            self.process_messages(&ps[..], rx),
            run_plugins(&ps[..], tx),
        )?;

        log::error!("golem exited");
        Ok(())
    }

    async fn recv_irc_messages(&self, tx: mpsc::Sender<Message>) -> Result<()> {
        let mut stream = {
            let mut client = self.irc_client.lock().unwrap();
            client.stream()?
        };

        while let Some(irc_message) = stream.next().await.transpose()? {
            tx.send(irc_message).await?
        }
        Ok(())
    }

    async fn process_messages(
        &self,
        ps: &[Box<dyn Plugin>],
        mut rx: mpsc::Receiver<Message>,
    ) -> Result<()> {
        while let Some(irc_message) = rx.recv().await {
            self.process_message(&ps, irc_message).await?;
        }
        Ok(())
    }

    async fn process_message(&self, ps: &[Box<dyn Plugin>], irc_message: Message) -> Result<()> {
        log::debug!("Got an irc message: {:?}", irc_message);
        // TODO don't crash if a plugin returns an error
        let messages = plugins_in_messages(&ps, &irc_message)
            .await
            .with_context(|| "Plugin error !")?;
        let client = self.irc_client.lock().expect("lock golem irc client");
        for msg in messages {
            if let Some(msg) = msg {
                // TODO don't crash if a plugin returns an error
                futures::stream::iter(ps.iter())
                    .map(Ok)
                    .try_for_each_concurrent(5, |plugin| {
                        let msg = &msg;
                        async move {
                            plugin.out_message(&msg).await?;
                            Ok::<(), anyhow::Error>(())
                        }
                    })
                    .await?;
                client.send(msg)?;
            }
        }

        Ok(())
    }
}

async fn plugins_in_messages(
    plugins: &[Box<dyn Plugin>],
    msg: &Message,
) -> Result<Vec<Option<Message>>> {
    let mut results = Vec::with_capacity(plugins.len());

    let (txs, rxs): (Vec<_>, Vec<_>) = plugins.iter().map(|_| oneshot::channel()).unzip();

    futures::stream::iter(plugins.iter().zip(txs))
        .map(Ok)
        .try_for_each_concurrent(5, |(plugin, tx)| async move {
            let mb_msg = plugin
                .in_message(msg)
                .await
                .with_context(|| format!("in_message error from plugin {}", plugin.get_name()))?;
            if let Err(_) = tx.send(mb_msg) {
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

async fn run_plugins(plugins: &[Box<dyn Plugin>], tx: mpsc::Sender<Message>) -> Result<()> {
    let x = plugins.iter().map(|p| {
        let tx = tx.clone();
        async move {
            p.run(tx)
                .await
                .with_context(|| format!("Plugin {}.run() failed", p.get_name()))?;
            Ok::<(), anyhow::Error>(())
        }
    });
    futures::future::try_join_all(x).await?;
    Ok(())
}
