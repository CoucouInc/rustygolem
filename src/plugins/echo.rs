use std::time::Duration;

use crate::plugin::{Plugin, Result};
use async_trait::async_trait;
use irc::proto::{Command, Message};
use tokio::sync::mpsc;

pub struct Echo {}

#[async_trait]
impl Plugin for Echo {
    async fn init() -> Result<Self> {
        Ok(Echo {})
    }

    fn get_name(&self) -> &'static str {
        "echo"
    }

    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        in_msg(msg).await
    }

    async fn run(&self, _bot_chan: mpsc::Sender<Message>) -> Result<()> {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            log::info!("echo plugin still running");
        }
    }
}

async fn in_msg(msg: &Message) -> Result<Option<Message>> {
    if let Command::PRIVMSG(_source, message) = &msg.command {
        Ok(msg.response_target().map(|target| {
            Command::PRIVMSG(target.to_string(), format!("echo - {}", message)).into()
        }))
    } else {
        Ok(None)
    }
}
