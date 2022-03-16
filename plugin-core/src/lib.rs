#![allow(unused_variables)]
use std::path::Path;

use async_trait::async_trait;
use irc::proto::Message;
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum Error {
    /// useful when constructing an error from scratch
    #[error("Generic plugin error {0}")]
    Synthetic(String),

    #[error("Plugin error from {source:?}")]
    Wrapped {
        source: Box<dyn std::error::Error + Send + Sync>,
        ctx: String,
    },

    #[error("Generic error")]
    Generic(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// Can't figure out how to automatically convert an Error (+ other bounds)
// into my plugin::Error, so instead, create this trait to do it.
pub trait WrapError<T> {
    fn wrap(self) -> Result<T>;
}

#[async_trait]
pub trait Plugin: Sync + Send {
    async fn init(config_path: &str) -> Result<Self>
    where
        Self: Sized;

    /// This method is polled (through .await) after initialisation once the bot is running.
    /// The given bot_chan can be used to send message to IRC out of band,
    /// that is, not as a response to an incoming event.
    /// This method can also be used to start an async process.
    async fn run(&self, bot_chan: mpsc::Sender<Message>) -> Result<()> {
        Ok(())
    }

    /// The unique identifier of the plugin
    fn get_name(&self) -> &'static str;

    /// Method invoked whenever a message is received from IRC
    /// Returns Some(Message) if a response message should be sent, None otherwise
    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        Ok(None)
    }

    /// Method invoked whenever the bot sends a message to IRC.
    async fn out_message(&self, msg: &Message) -> Result<()> {
        Ok(())
    }
}

pub async fn new_boxed<T>(config_path: &str) -> Result<Box<dyn Plugin>>
where
    T: Plugin + 'static,
{
    Ok(Box::new(T::init(config_path).await?))
}
