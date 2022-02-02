#![allow(clippy::upper_case_acronyms)]
use crate::plugin::{Plugin, Result};
use async_trait::async_trait;
use chrono::{format::StrftimeItems, Utc};
use irc::proto::{Command, Message};
use nom::branch::alt;
use nom::bytes::complete::{is_not, tag};
use nom::character::complete::{char, multispace0, multispace1};
use nom::combinator::{all_consuming, flat_map, map, opt, recognize};
use nom::sequence::{delimited, pair, preceded, terminated};
use nom::Finish;
use nom::IResult;

use crate::republican_calendar::RepublicanDate;

pub struct Ctcp {}

#[async_trait]
impl Plugin for Ctcp {
    async fn init() -> Result<Self> {
        Ok(Ctcp {})
    }

    fn get_name(&self) -> &'static str {
        "ctcp"
    }

    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        in_msg(msg).await
    }
}

async fn in_msg(msg: &Message) -> Result<Option<Message>> {
    let response_target = match msg.response_target() {
        None => return Ok(None),
        Some(target) => target.to_string(),
    };

    if let Command::PRIVMSG(_source, message) = &msg.command {
        // 🤮 the error handling isn't great there
        let command = match parse_command(message) {
            Some(x) => x,
            None => return Ok(None),
        };
        let msg = match command {
            CtcpCmd::VERSION => "rustygolem".to_string(),
            CtcpCmd::TIME => {
                let now = Utc::now();
                let fmt = StrftimeItems::new("%H:%M:%S");
                let rd = RepublicanDate::try_from(now.naive_utc().date())?;
                format!("TIME {} UTC - {}", now.format_with_items(fmt), rd)
            }
            CtcpCmd::PING(opt_arg) => {
                let arg = opt_arg
                    .map(|c| format!(" {}", c))
                    .unwrap_or_else(|| "".to_string());
                format!("PING{}", arg)
            }
        };

        let irc_msg = Command::PRIVMSG(response_target, msg).into();
        return Ok(Some(irc_msg));
    }

    Ok(None)
}

#[derive(Debug, PartialEq)]
enum CtcpCmd<'input> {
    VERSION,
    TIME,
    PING(Option<&'input str>),
}

fn parse_command(input: &str) -> Option<CtcpCmd<'_>> {
    all_consuming(terminated(parse_ctcp, multispace0))(input)
        .finish()
        .map(|x| x.1)
        .ok()
}

fn parse_ctcp(input: &str) -> IResult<&str, CtcpCmd<'_>> {
    let c = '\u{0001}';

    let raw_parse = delimited(char(c), is_not("\x01"), char(c));
    map(
        // sketchy flat_map there, there is likely a better combinator.
        flat_map(raw_parse, move |i| move |_| ctcp_cmd(i)),
        |x| x,
    )(input)
}

fn ctcp_cmd(input: &str) -> IResult<&str, CtcpCmd> {
    alt((
        map(tag("VERSION"), |_| CtcpCmd::VERSION),
        map(tag("TIME"), |_| CtcpCmd::TIME),
        map(
            pair(
                tag("PING"),
                opt(preceded(multispace1, recognize(is_not("\x01")))),
            ),
            |(_, arg)| CtcpCmd::PING(arg),
        ),
    ))(input)
}

// // ctcp feature is disabled so we can override the TIME to reply with
// // the republican calendar (crucial feature right there).
// fn handle_ctcp(client: &Arc<Mutex<Client>>, target: String, ctcp: CTCP) -> Result<()> {
//     let msg = match ctcp {
//         CTCP::VERSION => "VERSION rustygolem".to_string(),
//         CTCP::TIME => {
//             let now = Utc::now();
//             let fmt = StrftimeItems::new("%H:%M:%S");
//             let rd = RepublicanDate::try_from(now.naive_utc().date())?;
//             format!("TIME {} UTC - {}", now.format_with_items(fmt), rd)
//         }
//         CTCP::PING(opt_arg) => {
//             let arg = opt_arg
//                 .map(|c| format!(" {}", c))
//                 .unwrap_or_else(|| "".to_string());
//             format!("PING{}", arg)
//         }
//     };
//     {
//         let client = client.lock().unwrap();
//         client.send(Command::NOTICE(target, format!("\u{001}{}\u{001}", msg)))?;
//     }
//     Ok(())
// }
