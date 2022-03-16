use plugin_core::{Plugin, Result};
use crate::utils::parser;
use async_trait::async_trait;
use irc::proto::{Command, Message};

pub struct Joke {}

#[async_trait]
impl Plugin for Joke {
    async fn init(_config_path: &str) -> Result<Self> {
        Ok(Joke {})
    }

    fn get_name(&self) -> &'static str {
        "joke"
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
        if let Some(mb_target) = parser::single_command("joke", privmsg) {
            let msg = handle_command(mb_target)
                .await
                .unwrap_or_else(|| "Error handling joke".to_string());

            return Ok(Some(
                Command::PRIVMSG(response_target.to_string(), msg).into(),
            ));
        }
    }
    Ok(None)
}

async fn handle_command(mb_target: Option<&str>) -> Option<String> {
    let req = reqwest::Client::new()
        .get("https://icanhazdadjoke.com")
        .header("Accept", "text/plain");
    let resp = match req.send().await {
        Ok(r) => r,
        Err(err) => {
            return Some(format!(
                "Error while querying icanhazdadjoke API: {:?}",
                err
            ))
        }
    };

    let joke = match resp.text().await {
        Ok(t) => t,
        Err(err) => {
            return Some(format!(
                "Error while getting the response from icanhazdadjoke: {:?}",
                err
            ))
        }
    };

    Some(crate::utils::messages::with_target(&joke, &mb_target))
}
