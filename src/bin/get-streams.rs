use anyhow::{anyhow, Result};
use async_compat::Compat;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT},
    Client, StatusCode,
};
use serde::Deserialize;
use std::borrow::Cow;
use twitch_gift_farm::Config;

const KRAKEN_STREAMS: &str = "https://api.twitch.tv/kraken/streams/";
const APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const CLIENT_ID: &str = "34afn666979w6kmmr6b1bcnagfv6s3";

#[derive(Debug, Deserialize)]
struct StreamsResponse<'a> {
    streams: Vec<Stream<'a>>,
}

#[derive(Debug, Deserialize)]
struct Stream<'a> {
    channel: Channel<'a>,
}

#[derive(Debug, Deserialize)]
struct Channel<'a> {
    name: Cow<'a, str>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse<'a> {
    error: Cow<'a, str>,
    status: u16,
    message: Cow<'a, str>,
}

async fn get_streams_page<'a>(client: &Client, offset: u16) -> Result<Vec<Cow<'a, str>>> {
    Compat::new(async {
        let resp = client
            .get(KRAKEN_STREAMS)
            .query(&[("offset", offset), ("limit", 100)])
            .send()
            .await?;

        if resp.status() == StatusCode::BAD_REQUEST {
            let error = resp.json::<ErrorResponse>().await?;
            return Err(anyhow!("Could not get streams: {}", error.message));
        }

        let streams = resp
            .error_for_status()?
            .json::<StreamsResponse>()
            .await?
            .streams
            .into_iter()
            .map(|stream| stream.channel.name)
            .collect();

        Ok(streams)
    })
    .await
}

async fn get_streams<'a>() -> Result<Vec<Cow<'a, str>>> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.twitchtv.v5+json"),
    );
    headers.insert(
        HeaderName::from_static("client-id"),
        HeaderValue::from_static(CLIENT_ID),
    );

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .user_agent(APP_USER_AGENT)
        .build()?;

    let mut streams = Vec::with_capacity(1000);

    for i in 0..=9 {
        let offset = i * 100;
        streams.append(&mut get_streams_page(&client, offset).await?);
    }

    Ok(streams)
}

fn main() -> Result<()> {
    println!("Getting 1000 live channels");

    let mut channels = smol::block_on(get_streams())?;

    println!("Found {} channels", channels.len());

    let mut config = Config::load()?;
    let old_count = config.channels.len();

    config.channels.append(&mut channels);
    config.channels.sort();
    config.channels.dedup();

    println!(
        "Saving {} new channels for a total of {}",
        config.channels.len() - old_count,
        config.channels.len()
    );

    config.save()?;

    Ok(())
}
