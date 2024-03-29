use encoding_rs::{CoderResult, Encoding};
use google_youtube3::api::{PlaylistListResponse, SearchListResponse, VideoListResponse};
use mime::Mime;
use reqwest::header::HeaderValue;
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use irc::proto::{Command, Message};
use nom::{
    branch::alt,
    bytes::complete::{tag, take_till1, take_while, take_while1},
    character::complete::{digit1, multispace0, multispace1},
    combinator::{all_consuming, map, opt},
    multi::separated_list0,
    sequence::{delimited, pair, preceded, terminated, tuple},
    AsChar, Finish, IResult, InputTakeAtPosition,
};
use parking_lot::Mutex;
use plugin_core::{Error, Initialised, Plugin, Result};
use url::Url;

mod parsing_utils;

#[derive(Deserialize)]
struct YtConfig {
    youtube_api_key: Option<String>,
}

pub struct UrlPlugin {
    seen_urls: Arc<Mutex<HashMap<String, VecDeque<Url>>>>,
    client: reqwest::Client,
    yt_api_key: Option<String>,
}

impl UrlPlugin {
    fn new(config_path: &str) -> Result<Self> {
        // let path = "golem_config.dhall";
        let yt_config: YtConfig =
            serde_dhall::from_file(config_path)
                .parse()
                .map_err(|err| Error::Wrapped {
                    source: Box::new(err),
                    ctx: format!("Failed to read config at {config_path}"),
                })?;
        if yt_config.youtube_api_key.is_some() {
            log::info!("Url plugin initialized with youtube api credentials.");
        } else {
            log::warn!("Url plugin is missing youtube api key.");
        }

        Ok(UrlPlugin {
            seen_urls: Default::default(),
            client: reqwest::Client::new(),
            yt_api_key: yt_config.youtube_api_key,
        })
    }

    fn add_urls(&self, channel: &str, urls: Vec<Url>) {
        let mut seen_urls = self.seen_urls.lock();
        let e = seen_urls.entry(channel.to_string()).or_default();
        for url in urls {
            log::info!("Adding {url} to chan {channel}");
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
                match cmd {
                    Cmd::Url(mb_idx, mb_target) => {
                        let channel = match msg.response_target() {
                            None => return Ok(None),
                            Some(target) => target,
                        };
                        let message = self.get_url(channel, mb_idx.unwrap_or(0)).await?;

                        let target = mb_target.map(|t| format!("{t}: ")).unwrap_or_default();
                        let msg = format!("{target}{message}");
                        return Ok(Some(Command::PRIVMSG(channel.to_string(), msg).into()));
                    }
                    Cmd::Search(term, _mb_target) => {
                        let channel = match msg.response_target() {
                            None => return Ok(None),
                            Some(target) => target,
                        };
                        log::info!("searching yt for term {term}");
                        let msg = self.yt_search(term).await?;
                        return Ok(Some(Command::PRIVMSG(channel.to_string(), msg).into()));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn get_url(&self, channel: &str, idx: usize) -> Result<String> {
        let mb_url = {
            let urls_guard = self.seen_urls.lock();
            urls_guard
                .get(channel)
                .and_then(|urls| urls.len().checked_sub(1 + idx).and_then(|i| urls.get(i)))
                // clone the url so that we can release the lock.
                // This avoid holding it across await points when fetching data for the url
                .cloned()
        };
        let url = match mb_url {
            Some(u) => u,
            None => return Ok(format!("No stored url found at index {idx}")),
        };

        match &self.yt_api_key {
            Some(yt_key) if is_yt_url(&url) => self.get_yt_url(&url, yt_key).await,
            _ => self.get_regular_url(&url).await,
        }
    }

    async fn get_regular_url(&self, url: &Url) -> Result<String> {
        log::info!("Querying url {}", url);
        let resp = self
            .client
            .get(url.clone())
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(err) => return Ok(format!("Problème avec l'url {}: {}", url, err)),
        };

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

        self.sniff_title(resp).await
    }

    // To avoid someone pointing the bot at a gigantic file, filling up memory or disk
    async fn sniff_title(&self, resp: reqwest::Response) -> Result<String> {
        sniff_title(resp).await
    }

    async fn get_yt_url(&self, url: &Url, yt_api_key: &str) -> Result<String> {
        let yt_id = match extract_yt_id(url) {
            Some(x) => x,
            None => {
                return Ok(format!(
                    "Ook Ook 🙈, pas possible de trouver quoi query pour {}",
                    url
                ))
            }
        };

        log::debug!("fetching yt data for {yt_id:?}");
        match yt_id {
            YtId::Video(vid_id) => {
                let vids: VideoListResponse =
                    self.yt_api_call(yt_api_key, "videos", &vid_id).await?;
                match vids.items.unwrap_or_default().first() {
                    Some(vid) => {
                        let snip = vid.snippet.as_ref().unwrap();
                        let title = snip.title.as_deref().unwrap_or("");
                        let chan = snip.channel_title.as_deref().unwrap_or("");
                        let published_at = snip
                            .published_at
                            .as_deref()
                            .map(|d| format!(" - {d}"))
                            .unwrap_or_else(|| "".to_string());
                        Ok(format!(
                            "{} [{}{}] [{}]",
                            &title, &chan, &published_at, &url
                        ))
                    }
                    None => Ok(format!("Rien trouvé pour vidéo {vid_id}")),
                }
            }
            YtId::Channel(chan_name) => {
                let raw_resp = self
                    .client
                    .get("https://www.googleapis.com/youtube/v3/search")
                    .query(&[("key", yt_api_key)])
                    .query(&[("part", "snippet")])
                    .query(&[("type", "channel")])
                    .query(&[("q", chan_name)])
                    .send()
                    .await
                    .map_err(|err| Error::Wrapped {
                        source: Box::new(err),
                        ctx: format!("Failed to fetch channel with id {chan_name}"),
                    })?;

                if raw_resp.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(format!("Pas trouvé de chan pour {chan_name}"));
                }

                if raw_resp.status() != reqwest::StatusCode::OK {
                    return Ok(format!("Ooops, status code: {}", raw_resp.status()));
                }

                let results: SearchListResponse =
                    raw_resp.json().await.map_err(|err| Error::Wrapped {
                        source: Box::new(err),
                        ctx: format!("Cannot parse response when fetching channel {chan_name}"),
                    })?;

                match results.items.unwrap_or_default().first() {
                    Some(search_result) => {
                        let snip = search_result.snippet.as_ref().unwrap();
                        let title = snip.channel_title.as_deref().unwrap_or("");
                        let description = snip.description.as_deref().unwrap_or("");
                        let published_at = snip
                            .published_at
                            .as_deref()
                            .map(|d| format!(" - {d}"))
                            .unwrap_or_else(|| "".to_string());
                        if description.is_empty() {
                            Ok(format!("Channel: {}{} [{}]", title, published_at, url))
                        } else {
                            Ok(format!(
                                "Channel: {}{} ({}) [{}]",
                                title, published_at, description, url
                            ))
                        }
                    }
                    None => Ok(format!("Pas trouvé de chan pour {chan_name}")),
                }
            }
            YtId::Playlist(playlist_id) => {
                let playlists: PlaylistListResponse = self
                    .yt_api_call(yt_api_key, "playlists", &playlist_id)
                    .await?;
                match playlists.items.unwrap_or_default().first() {
                    Some(playlist) => {
                        let snip = playlist.snippet.as_ref().unwrap();
                        let title = snip.title.as_deref().unwrap_or("");
                        Ok(format!("Playlist: {} [{}]", &title, &url))
                    }
                    None => Ok(format!("Pas de playlist trouvée pour {playlist_id}")),
                }
            }
        }
    }

    async fn yt_api_call<T, Q>(&self, yt_api_key: &str, resource: &str, resource_id: Q) -> Result<T>
    where
        T: DeserializeOwned,
        Q: serde::Serialize + std::fmt::Display,
    {
        let mut url = Url::parse("https://www.googleapis.com/youtube/v3").unwrap();
        url.path_segments_mut().unwrap().push(resource);

        self.client
            .get(url)
            .query(&[("id", &resource_id)])
            .query(&[("key", yt_api_key.to_owned())])
            .query(&[("part", "snippet")])
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .and_then(|x| x.error_for_status())
            .map_err(|err| Error::Wrapped {
                source: Box::new(err),
                ctx: format!("Failed to fetch {resource} with id {resource_id}"),
            })?
            .json()
            .await
            .map_err(|err| Error::Wrapped {
                source: Box::new(err),
                ctx: format!("Failed to fetch {resource} with id {resource_id}"),
            })
    }

    async fn yt_search(&self, search_term: &str) -> Result<String> {
        let key = match &self.yt_api_key {
            Some(k) => k,
            None => {
                return Ok(format!(
                    "No youtube api key provided, can't search: {search_term}"
                ))
            }
        };

        let raw_resp = self
            .client
            .get("https://www.googleapis.com/youtube/v3/search")
            .query(&[("key", key)])
            .query(&[("part", "snippet")])
            // .query(&[("type", "channel")])
            .query(&[("q", search_term)])
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|err| Error::Wrapped {
                source: Box::new(err),
                ctx: format!("Failed to search yt for {search_term}"),
            })?;

        let jsonbody: std::result::Result<SearchListResponse, _> = raw_resp.json().await;

        match jsonbody {
            Ok(search_resp) => match search_resp.items.as_ref().and_then(|v| v.first()) {
                Some(search_result) => {
                    let kind = search_result
                        .id
                        .as_ref()
                        .and_then(|x| x.kind.as_ref())
                        .unwrap();

                    match &kind[..] {
                        "youtube#channel" => {
                            let channel_id = search_result
                                .snippet
                                .as_ref()
                                .and_then(|x| x.channel_id.as_ref())
                                .unwrap();
                            let channel_title = search_result
                                .snippet
                                .as_ref()
                                .and_then(|x| x.channel_title.as_deref())
                                .unwrap_or("no channel found");
                            Ok(format!("channel: [{channel_title}] https://www.youtube.com/channel/{channel_id}"))
                        }
                        "youtube#playlist" => {
                            let title = search_result
                                .snippet
                                .as_ref()
                                .unwrap()
                                .title
                                .as_ref()
                                .unwrap();

                            let playlist_id = search_result
                                .id
                                .as_ref()
                                .and_then(|x| x.playlist_id.as_ref())
                                .unwrap();

                            Ok(format!("playlist: {title} https://www.youtube.com/playlist?list={playlist_id}"))
                        }
                        "youtube#video" => {
                            let title = search_result
                                .snippet
                                .as_ref()
                                .unwrap()
                                .title
                                .as_ref()
                                .unwrap();

                            let vid_id = search_result
                                .id
                                .as_ref()
                                .and_then(|x| x.video_id.as_ref())
                                .unwrap();

                            let channel_title = search_result
                                .snippet
                                .as_ref()
                                .and_then(|x| x.channel_title.as_deref())
                                .unwrap_or("no channel found");

                            Ok(format!("{title} [{channel_title}] https://www.youtube.com/watch?v={vid_id}"))
                        }
                        _ => return Ok(format!("Rien trouvé pour {search_term} /o\\")),
                    }
                }
                None => return Ok(format!("Rien trouvé pour {search_term} /o\\")),
            },
            Err(err) => {
                log::error!("Can't parse yt response for {search_term}\n{:?}", err);
                return Err(Error::Wrapped {
                    source: Box::new(err),
                    ctx: format!("Failed to parse json response for {search_term}"),
                });
            }
        }
    }
}

#[async_trait]
impl Plugin for UrlPlugin {
    async fn init(config: &plugin_core::Config) -> Result<Initialised> {
        let plugin = UrlPlugin::new(&config.config_path)?;
        Ok(Initialised::from(plugin))
    }

    fn get_name(&self) -> &'static str {
        "url"
    }

    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        self.in_msg(msg).await
    }

    fn ignore_blacklisted_users(&self) -> bool {
        false
    }
}

// all characters considered as space by the regex \s
const SPACE_CHARS: [char; 25] = [
    '\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{85}', '\u{a0}', '\u{1680}', '\u{2000}',
    '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}',
    '\u{2009}', '\u{200a}', '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
];

// this is a copy of multispace1 from nom, but expanded to also account
// for some additional characters
pub fn custom_multispace1<T, E: nom::error::ParseError<T>>(input: T) -> IResult<T, T, E>
where
    T: InputTakeAtPosition,
    <T as InputTakeAtPosition>::Item: AsChar + Clone,
{
    input.split_at_position1_complete(
        |item| !SPACE_CHARS.contains(&item.as_char()),
        nom::error::ErrorKind::MultiSpace,
    )
}

fn parse_urls<'a>(msg: &'a str) -> Result<Vec<Url>> {
    match separated_list0(custom_multispace1, parse_url)(msg) {
        Ok((_, urls)) => Ok(urls.into_iter().flatten().collect()),
        Err(_) => Err(plugin_core::Error::Synthetic(format!(
            "Cannot parse url from {msg}"
        ))),
    }
}

fn parse_url(raw: &str) -> IResult<&str, Option<Url>> {
    map(
        take_while(|c: char| !SPACE_CHARS.contains(&c)),
        |word| match Url::parse(word) {
            Ok(u) if !u.cannot_be_a_base() && (u.scheme() == "http" || u.scheme() == "https") => {
                Some(u)
            }
            _ => None,
        },
    )(raw)
}

#[derive(PartialEq, Eq, Debug)]
enum Cmd<'msg> {
    /// optional url index, optional target nick
    Url(Option<usize>, Option<&'msg str>),
    /// search term, optional target nick
    Search(&'msg str, Option<&'msg str>),
}

/// returns Option<(optional_url_index, optional_target_nick)>
fn parse_command(msg: &str) -> Option<Cmd<'_>> {
    let cmd = preceded(
        parsing_utils::command_prefix,
        alt((
            map(
                parsing_utils::with_target(pair(tag("url"), opt(preceded(multispace1, digit1)))),
                |((_, mb_idx), mb_target)| {
                    let idx = mb_idx.and_then(|raw| str::parse(raw).ok());
                    Cmd::Url(idx, mb_target)
                },
            ),
            map(
                preceded(
                    pair(tag("yt_search"), multispace1),
                    alt((
                        map(
                            tuple((
                                take_till1(|c| c == '>'),
                                delimited(
                                    pair(nom::character::complete::char('>'), multispace0),
                                    parsing_utils::word,
                                    multispace0,
                                ),
                            )),
                            |(x, t)| (x, Some(t)),
                        ),
                        map(
                            terminated(take_while1(|c| c != '>'), nom::combinator::eof),
                            |x| (x, None),
                        ),
                    )),
                ),
                |(x, t)| Cmd::Search(x, t),
            ),
        )),
    );
    all_consuming(terminated(cmd, multispace0))(msg)
        .finish()
        .map(|x| x.1)
        .ok()
}

const YT_HOSTNAMES: [&str; 5] = [
    "youtube.com",
    "www.youtube.com",
    "youtu.be",
    "www.youtu.be",
    "m.youtube.com",
];

fn is_yt_url(url: &Url) -> bool {
    url.host()
        .map(|h| match h {
            url::Host::Domain(domain) => YT_HOSTNAMES.contains(&domain),
            url::Host::Ipv4(_) | url::Host::Ipv6(_) => false,
        })
        .unwrap_or(false)
}

#[derive(PartialEq, Eq, Debug)]
enum YtId<'url> {
    Video(Cow<'url, str>),
    Channel(&'url str),
    Playlist(Cow<'url, str>),
}

fn extract_yt_id(url: &Url) -> Option<YtId<'_>> {
    let mut segments = url.path_segments()?;
    let first_segment = segments.next();
    let second_segment = segments.next();

    if matches!(url.host(), Some(url::Host::Domain("youtu.be"))) {
        return first_segment.map(|v| YtId::Video(Cow::Borrowed(v)));
    }

    match first_segment {
        Some("c") | Some("channel") | Some("user") => second_segment.map(YtId::Channel),
        Some("watch") => {
            url.query_pairs()
                .find_map(|(k, v)| if k == "v" { Some(YtId::Video(v)) } else { None })
        }
        Some("shorts") => second_segment.map(|v| YtId::Video(Cow::Borrowed(v))),
        Some("playlist") => url.query_pairs().find_map(|(k, v)| {
            if k == "list" {
                Some(YtId::Playlist(v))
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// This is copy pasted and adapted from the method with the same name in reqwest:
/// https://docs.rs/reqwest/latest/src/reqwest/async_impl/response.rs.html#184-207
/// The difference is about reading only the beginning of the response up to a point
/// to avoid a denial of service where the bot is pointed at a 100GB response.
/// Defaults to utf-8
fn text_with_charset(bytes: &[u8], content_type: &Option<HeaderValue>) -> Result<String> {
    let ct = content_type
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<Mime>().ok());

    let mut decoder = ct
        .as_ref()
        .and_then(|mime| mime.get_param("charset").map(|charset| charset.as_str()))
        .and_then(|encoding_name| Encoding::for_label(encoding_name.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8)
        .new_decoder();

    // let mut decoder = Encoding::for_label(b"utf-8").unwrap().new_decoder();
    // let (res, byte_read, did_replace) =
    //     decoder.decode_to_string(&buffer, &mut dst, reached_end_of_stream);

    let mut dst = String::with_capacity(5 * 1024);
    let (res, _byte_read, _did_replace) = decoder.decode_to_string(bytes, &mut dst, false);

    // because res is #[must_use]
    match res {
        CoderResult::InputEmpty => (),
        CoderResult::OutputFull => (),
    }
    Ok(dst)
}

pub async fn sniff_title(mut resp: reqwest::Response) -> Result<String> {
    let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let url = resp.url().to_string();

    // only bother to look further if the content type looks like html or text
    match ct.as_ref().and_then(|h| h.to_str().ok()) {
        Some(ct) if ct.contains("text") || ct.contains("html") => (),
        Some(ct) => {
            return Ok(format!(
                "Cannot extract title from content type {ct} for {url}",
            ))
        }
        _ => return Ok(format!("No valid content type found for {url}")),
    };

    // don't download more than `capa` bytes (to avoid dos)
    let capa = 10 * 1024;
    let mut read_buf = bytes::BytesMut::with_capacity(capa);

    while let Some(chunk) = resp.chunk().await.transpose() {
        let chunk = chunk.map_err(|err| Error::Wrapped {
            source: Box::new(err),
            ctx: format!("Failed to read bytes from response for url {}", url),
        })?;

        // make sure we don't read more than the allocated capacity
        let l = (capa - read_buf.len()).min(chunk.len());
        read_buf.extend_from_slice(&chunk[0..l]);
        if read_buf.len() >= capa {
            break;
        }
    }

    // <title data-rh=\"true\">Greta Thunberg carried away by police at German mine protest | AP News</title>
    let fragment = text_with_charset(&read_buf, &ct)?;

    let selector = scraper::Selector::parse("title").unwrap();
    // there can be a problem since `<title>coucou` is parsed as the
    // full title. So need to grab enough bytes from the network
    // to be reasonably sure that we got the full title
    // Also, ignore any parse error. The parser is very lenient and can
    // gives us a title even if there are other error in the document
    if let Some(title) = scraper::Html::parse_document(&fragment)
        .select(&selector)
        .next()
    {
        log::debug!("found title: {title:?}");
        let title = title
            .text()
            .into_iter()
            .collect::<String>()
            .replace('\n', " ");

        // Simply slicing the string like title[..100] will panic if
        // it stops across an utf-8 codepoint boundary.
        // So need to iterate across real chars to split properly.
        let char_len = title.chars().count();
        if char_len > 100 {
            let f = title.chars().take(100).collect::<String>();
            Ok(format!("{}[…] [{url}]", f))
        } else {
            Ok(format!("{title} [{url}]"))
        }
    } else {
        Ok(format!("No title found at {url}"))
    }
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

        assert_eq!(
            parse_urls("some special chars : http://nbsp.com").unwrap(),
            vec![Url::parse("http://nbsp.com").unwrap()]
        )
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
        assert_eq!(parse_command("λurl"), Some(Cmd::Url(None, None)));
    }

    #[test]
    fn test_command_with_idx() {
        assert_eq!(parse_command("λurl 2"), Some(Cmd::Url(Some(2), None)));
    }

    #[test]
    fn test_command_with_target() {
        assert_eq!(
            parse_command("λurl > charlie"),
            Some(Cmd::Url(None, Some("charlie")))
        );
    }

    #[test]
    fn test_command_with_idx_and_target() {
        assert_eq!(
            parse_command("λurl 3 > charlie"),
            Some(Cmd::Url(Some(3), Some("charlie")))
        );
    }

    #[test]
    fn test_command_search_with_target() {
        assert_eq!(
            parse_command("λyt_search coucou1 and coucou2 > charlie"),
            Some(Cmd::Search("coucou1 and coucou2 ", Some("charlie")))
        );
    }

    fn grmbl_till(raw: &str) -> IResult<&str, &str> {
        terminated(
            take_while1(|c| c != '>'),
            tuple((
                nom::character::complete::char('>'),
                multispace0,
                parsing_utils::word,
                multispace0,
                nom::combinator::eof,
            )),
        )(raw)
        // rest(raw)
    }

    #[test]
    fn test_take_till() {
        let input = "coucou > blah";
        let res = all_consuming(grmbl_till)(input).finish().ok();
        assert_eq!(res, Some(("", "coucou ")));
    }

    #[test]
    fn test_command_search_multi_word() {
        assert_eq!(
            parse_command("λyt_search coucou and charlie"),
            Some(Cmd::Search("coucou and charlie", None))
        );
    }

    #[test]
    fn test_command_search_missing_search() {
        assert_eq!(parse_command("λyt_search"), None);
    }

    #[test]
    fn test_command_search_missing_search_with_target() {
        assert_eq!(parse_command("λyt_search > charlie"), None);
    }

    #[test]
    fn test_command_search() {
        assert_eq!(
            parse_command("λyt_search coucou"),
            Some(Cmd::Search("coucou", None))
        );
    }

    #[test]
    fn test_is_yt_url() {
        assert!(!is_yt_url(
            &Url::parse("https://github.com/CoucouInc/rustygolem").unwrap()
        ));

        assert!(is_yt_url(
            &Url::parse("https://youtube.com/c/BosnianApeSociety").unwrap()
        ));

        assert!(is_yt_url(
            &Url::parse("https://www.youtube.com/watch?v=0F5GQAnj0lo").unwrap()
        ));

        assert!(is_yt_url(
            &Url::parse("https://youtu.be/haLBM94SENg?t=256").unwrap()
        ));

        assert!(is_yt_url(
            &Url::parse("https://m.youtube.com/watch?v=haLBM94SENg").unwrap()
        ));

        // https://m.youtube.com/watch?list=PLJcTRymdlUQPwx8qU4ln83huPx-6Y3XxH&v=5MKjPYuD60I&feature=emb_imp_woyt]
    }

    #[test]
    fn test_extract_yt_id() {
        assert_eq!(
            extract_yt_id(&Url::parse("https://github.com/CoucouInc/rustygolem").unwrap()),
            None
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://www.youtube.com/results?search_query=mj").unwrap()),
            None
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://youtu.be/6gwBOTggfRc").unwrap()),
            Some(YtId::Video("6gwBOTggfRc".into()))
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://www.youtube.com/watch?v=ZZ3F3zWiEmc").unwrap()),
            Some(YtId::Video("ZZ3F3zWiEmc".into()))
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://www.youtube.com/shorts/EU4p-OC4O3o").unwrap()),
            Some(YtId::Video("EU4p-OC4O3o".into()))
        );

        assert_eq!(
            extract_yt_id(
                &Url::parse("https://www.youtube.com/c/%E3%81%8B%E3%82%89%E3%82%81%E3%82%8B")
                    .unwrap()
            ),
            // からめる
            Some(YtId::Channel("%E3%81%8B%E3%82%89%E3%82%81%E3%82%8B"))
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://www.youtube.com/c/inanutshell").unwrap()),
            Some(YtId::Channel("inanutshell"))
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://www.youtube.com/c/inanutshell/videos").unwrap()),
            Some(YtId::Channel("inanutshell"))
        );

        assert_eq!(
            extract_yt_id(
                &Url::parse("https://www.youtube.com/channel/UCworsKCR-Sx6R6-BnIjS2MA").unwrap()
            ),
            Some(YtId::Channel("UCworsKCR-Sx6R6-BnIjS2MA"))
        );

        assert_eq!(
            extract_yt_id(&Url::parse("https://youtube.com/c/BosnianApeSociety").unwrap()),
            Some(YtId::Channel("BosnianApeSociety"))
        );

        assert_eq!(
            extract_yt_id(
                &Url::parse(
                    "https://www.youtube.com/playlist?list=PLoBxKk9n0UWcv0HTYARFyCb0s9P21cDSd"
                )
                .unwrap()
            ),
            Some(YtId::Playlist("PLoBxKk9n0UWcv0HTYARFyCb0s9P21cDSd".into()))
        );

        //

        assert_eq!(
            extract_yt_id(&Url::parse("https://www.youtube.com/user/VieDeChouhartem").unwrap()),
            Some(YtId::Channel("VieDeChouhartem"))
        );
    }

    #[test]
    fn test_decode_text() {
        let sparkle_heart = vec![240, 159, 146, 150];
        assert_eq!(
            text_with_charset(&sparkle_heart, &None).unwrap(),
            "💖".to_string()
        );
    }
}
