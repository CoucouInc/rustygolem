use anyhow::Result;
use chrono::{format::StrftimeItems, Utc};
use irc::{client::Client, proto::Command};
use std::convert::TryFrom;
use std::sync::{Arc, Mutex};

use crate::{parser::CTCP, republican_calendar::RepublicanDate};

// ctcp feature is disabled so we can override the TIME to reply with
// the republican calendar (crucial feature right there).
pub(crate) fn handle_ctcp(client: &Arc<Mutex<Client>>, target: String, ctcp: CTCP) -> Result<()> {
    let msg = match ctcp {
        CTCP::VERSION => "VERSION rustygolem".to_string(),
        CTCP::TIME => {
            let now = Utc::now();
            let fmt = StrftimeItems::new("%H:%M:%S");
            let rd = RepublicanDate::try_from(now.naive_utc().date())?;
            format!("TIME {} UTC - {}", now.format_with_items(fmt), rd)
        }
        CTCP::PING(opt_arg) => {
            let arg = opt_arg.map(|c| format!(" {}", c)).unwrap_or("".to_string());
            format!("PING{}", arg)
        }
    };
    {
        let client = client.lock().unwrap();
        client.send(Command::NOTICE(target, format!("\u{001}{}\u{001}", msg)))?;
    }
    Ok(())
}
