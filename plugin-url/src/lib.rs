use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use async_trait::async_trait;
use irc::proto::{Command, Message};
use nom::{
    bytes::complete::{tag, take_while},
    character::complete::{digit1, multispace0, multispace1},
    combinator::{all_consuming, map, opt},
    multi::separated_list0,
    sequence::{pair, preceded, terminated},
    Finish, IResult,
};
use parking_lot::Mutex;
use plugin_core::{Error, Plugin, Result};
use url::Url;

mod parsing_utils;

pub struct UrlPlugin {
    seen_urls: Arc<Mutex<HashMap<String, VecDeque<Url>>>>,
}

impl UrlPlugin {
    fn new() -> Self {
        UrlPlugin {
            seen_urls: Default::default(),
        }
    }

    fn add_urls(&self, channel: &str, urls: Vec<Url>) {
        // log::debug!("Adding urls to chan: {channel} {urls:?}");
        let mut seen_urls = self.seen_urls.lock();
        let e = seen_urls.entry(channel.to_string()).or_default();
        for url in urls {
            log::debug!("Adding url to chan: {url}");
            e.push_back(url);
            if e.len() > 10 {
                e.pop_front();
            }
        }
    }

    async fn in_msg(&self, msg: &Message) -> Result<Option<Message>> {
        if let Command::PRIVMSG(source, privmsg) = &msg.command {
            self.add_urls(source, parse_urls(privmsg)?);

            if let Some(cmd) = parse_command(privmsg) {
                let (mb_idx, mb_target) = cmd;
                let channel = match msg.response_target() {
                    None => return Ok(None),
                    Some(target) => target,
                };
                let message = self.get_url(channel, mb_idx.unwrap_or(0)).await?;

                let target = mb_target.map(|t| format!("{t}: ")).unwrap_or_default();
                let msg = format!("{target}{message}");
                return Ok(Some(Command::PRIVMSG(channel.to_string(), msg).into()));
            }
        }
        Ok(None)
    }

    async fn get_url(&self, channel: &str, idx: usize) -> Result<String> {
        let mb_url = {
            let urls_guard = self.seen_urls.lock();
            urls_guard
                .get(channel)
                .and_then(|urls| {
                    urls.get(urls.len() - 1 - idx)
                })
                // clone the url so that we can release the lock.
                // This avoid holding it across await points when fetching data for the url
                .cloned()
        };
        let url = match mb_url {
            Some(u) => u,
            None => return Ok(format!("No stored url found at index {idx}")),
        };

        let resp = reqwest::get(url.clone())
            .await
            .map_err(|err| Error::Wrapped {
                source: Box::new(err),
                ctx: format!("Cannot GET {url}"),
            })?;

        let status_code = resp.status();
        if status_code != reqwest::StatusCode::OK {
            return Ok(format!("Oops, wrong status code, got {}", status_code));
        }

        match resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
        {
            Some(ct) if ct.contains("text") || ct.contains("html") => (),
            Some(ct) => {
                return Ok(format!(
                    "Cannot extract title from content type {ct} for {url}"
                ))
            }
            _ => return Ok(format!("No valid content type found for {url}")),
        };

        let body = resp.text().await.map_err(|err| Error::Wrapped {
            source: Box::new(err),
            ctx: format!("Cannot extract body at {url}"),
        })?;

        let selector = scraper::Selector::parse("title").unwrap();
        if let Some(title) = scraper::Html::parse_document(&body)
            .select(&selector)
            .next()
        {
            let title = title.text().into_iter().collect::<String>();
            Ok(format!("{title} [{url}]"))
        } else {
            Ok(format!("No title found at {url}"))
        }
    }
}

#[async_trait]
impl Plugin for UrlPlugin {
    async fn init() -> Result<Self> {
        Ok(UrlPlugin::new())
    }

    fn get_name(&self) -> &'static str {
        "url"
    }

    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        self.in_msg(msg).await
    }
}

fn parse_urls(msg: &str) -> Result<Vec<Url>> {
    match separated_list0(multispace1, parse_url)(msg) {
        Ok((_, urls)) => Ok(urls.into_iter().flatten().collect()),
        Err(_) => Err(plugin_core::Error::Synthetic(format!(
            "Cannot parse url from {msg}"
        ))),
    }
}

fn parse_url(raw: &str) -> IResult<&str, Option<Url>> {
    map(
        take_while(|c: char| !(c == ' ' || c == '\t' || c == '\r' || c == '\n')),
        |word| Url::parse(word).ok(),
    )(raw)
}

fn parse_command(msg: &str) -> Option<(Option<usize>, Option<&str>)> {
    let cmd = preceded(
        parsing_utils::command_prefix,
        map(
            parsing_utils::with_target(pair(tag("url"), opt(preceded(multispace1, digit1)))),
            |((_, mb_idx), mb_target)| {
                let idx = mb_idx.and_then(|raw| str::parse(raw).ok());
                (idx, mb_target)
            },
        ),
    );
    all_consuming(terminated(cmd, multispace0))(msg)
        .finish()
        .map(|x| x.1)
        .ok()
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_simple_url() {
        assert_eq!(
            parse_urls("http://coucou.com").unwrap(),
            vec![Url::parse("http://coucou.com").unwrap()]
        )
    }

    #[test]
    fn test_url_prefix() {
        assert_eq!(
            parse_urls("  http://coucou.com").unwrap(),
            vec![Url::parse("http://coucou.com").unwrap()]
        );
        assert_eq!(
            parse_urls("some stuff before  http://coucou.com").unwrap(),
            vec![Url::parse("http://coucou.com").unwrap()]
        );
    }

    #[test]
    fn test_url_suffix() {
        assert_eq!(
            parse_urls("http://coucou.com some stuff after").unwrap(),
            vec![Url::parse("http://coucou.com").unwrap()]
        );
    }

    #[test]
    fn test_url_surround() {
        assert_eq!(
            parse_urls("some stuff before http://coucou.com some stuff after").unwrap(),
            vec![Url::parse("http://coucou.com").unwrap()]
        );
    }

    #[test]
    fn test_weird_chars() {
        assert_eq!(
            parse_urls("http://coucou.com	taaaaabs").unwrap(),
            vec![Url::parse("http://coucou.com").unwrap()]
        );
    }

    #[test]
    fn test_multiple_urls() {
        assert_eq!(
            parse_urls("hello http://coucou.com some stuff and https://blah.foo.com to finish")
                .unwrap(),
            vec![
                Url::parse("http://coucou.com").unwrap(),
                Url::parse("https://blah.foo.com").unwrap(),
            ]
        );
    }

    #[test]
    fn test_simple_command_no_match() {
        assert_eq!(parse_command("λlol"), None);
    }

    #[test]
    fn test_simple_command() {
        assert_eq!(parse_command("λurl"), Some((None, None)));
    }

    #[test]
    fn test_command_with_idx() {
        assert_eq!(parse_command("λurl 2"), Some((Some(2), None)));
    }

    #[test]
    fn test_command_with_target() {
        assert_eq!(
            parse_command("λurl > charlie"),
            Some((None, Some("charlie")))
        );
    }

    #[test]
    fn test_command_with_idx_and_target() {
        assert_eq!(
            parse_command("λurl 3 > charlie"),
            Some((Some(3), Some("charlie")))
        );
    }
}
