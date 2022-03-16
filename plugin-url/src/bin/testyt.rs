#![allow(dead_code, unused_imports)]
use std::error::Error;

use google_youtube3 as yt3;
use reqwest::Url;
use serde::Deserialize;
use yt3::{
    api::{ChannelListResponse, PlaylistListResponse, VideoListResponse},
    hyper, hyper_rustls,
    oauth2::{self, ApplicationSecret},
    YouTube,
};

#[derive(Debug, Deserialize)]
struct YtConfig {
    youtube: Option<YtCreds>,
}

#[derive(Debug, Deserialize)]
struct YtCreds {
    client_id: String,
    api_key: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("YT_API_KEY").expect("YT_API_KEY env var not found");

    // Stuff to check:
    // * videos ✓
    // * channels ✓
    // * playlists ✓
    // * shorts ✓ (same as videos)

    // https://www.youtube.com/shorts/WrD_Uu5yHeY
    // https://www.youtube.com/channel/UCff0T5yQz5ezHfv6Djx3qOA
    // https://www.youtube.com/playlist?list=PLoBxKk9n0UWcv0HTYARFyCb0s9P21cDSd

    // (`T.isInfixOf` rawUrl)
    // [ "https://youtube.com",
    //   "https://www.youtube.com",
    //   "https://youtu.be",
    //   "https://www.youtu.be",
    //   "https://m.youtube.com"
    // ]

    let url = Url::parse("https://www.googleapis.com/youtube/v3/search")?;

    let q = std::env::var("SEARCH").unwrap_or_else(|_| "UCmwOm-YjCmcddE9xe9sEpUg".to_string());
    let resp = reqwest::Client::new()
        .get(url)
        // https://www.youtube.com/user/VieDeChouhartem
        // .query(&[("id", "Hio2lzkEUmo")])
        .query(&[("key", api_key)])
        .query(&[("part", "snippet")])
        .query(&[("type", "channel")])
        .query(&[("type", "video")])
        .query(&[("type", "playlist")])
        .query(&[("q", q)])
        .send()
        .await?
        .error_for_status()?;

    dbg!(&resp);
    // let jsonbody: VideoListResponse = resp.json().await?;
    let jsonbody: yt3::api::SearchListResponse = resp.json().await?;
    // let jsonbody: PlaylistListResponse = resp.json().await?;

    // for video:
    // title, channel_title

    let searchresult = &jsonbody.items.unwrap()[0];
    println!("{:#?}", searchresult);

    // println!("{:?}", resp.unwrap().text().await);
    //
    // println!("done");

    Ok(())
}
