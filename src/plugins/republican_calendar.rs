use crate::plugin::{Plugin, Result};
use crate::utils::parser;
use anyhow::Context;
use async_trait::async_trait;
use irc::proto::{Command, Message};

pub struct RepublicanCalendar {}

#[async_trait]
impl Plugin for RepublicanCalendar {
    async fn init() -> Result<Self> {
        Ok(RepublicanCalendar {})
    }

    fn get_name(&self) -> &'static str {
        "date"
    }

    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        in_msg(msg).await
    }
}

async fn in_msg(msg: &Message) -> Result<Option<Message>> {
    let response_target = match msg.response_target() {
        None => return Ok(None),
        Some(target) => target,
    };

    if let Command::PRIVMSG(_source, privmsg) = &msg.command {
        if let Some(mb_target) = parser::single_command("date", privmsg) {
            let msg = crate::republican_calendar::handle_command(mb_target)
                .context("republican calendar")?;

            return Ok(Some(Command::PRIVMSG(response_target.to_string(), msg).into()));
        }
    }
    Ok(None)
}