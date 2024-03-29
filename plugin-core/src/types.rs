#![allow(unused_variables)]

use async_trait::async_trait;
use irc::proto::Message;
use tokio::sync::mpsc;
use axum::Router;

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

pub struct Config {
    pub config_path: String,
}

pub struct Initialised {
    pub plugin: Box<dyn Plugin>,
    pub router: Option<Router>,
}

impl<T: Plugin + 'static> std::convert::From<T> for Initialised {
    fn from(value: T) -> Self {
        Initialised {
            plugin: Box::new(value),
            router: None,
        }
    }
}

#[async_trait]
pub trait Plugin: Sync + Send {
    async fn init(config: &Config) -> Result<Initialised>
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

    /// if the plugin should have a special handling for usually ignored users
    /// (typically, other bots), override this to return false.
    /// In this case `in_message` will also be invoked for messages coming from
    /// these blacklisted users.
    fn ignore_blacklisted_users(&self) -> bool {
        true
    }
}
