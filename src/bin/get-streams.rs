use anyhow::{anyhow, Result};
use async_compat::Compat;
use futures::future::try_join_all;
use log::info;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT},
    Client, StatusCode,
};
use serde::Deserialize;
use std::borrow::Cow;
use twitch_gift_farm::{logger_format, Config};

const KRAKEN_STREAMS: &str = "https://api.twitch.tv/kraken/streams";
const KRAKEN_TOP_GAMES: &str = "https://api.twitch.tv/kraken/games/top";
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
struct TopGamesResponse<'a> {
    top: Vec<Game<'a>>,
}

#[derive(Debug, Deserialize)]
struct Game<'a> {
    game: GameData<'a>,
}

#[derive(Debug, Deserialize)]
struct GameData<'a> {
    name: Cow<'a, str>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse<'a> {
    error: Cow<'a, str>,
    status: u16,
    message: Cow<'a, str>,
}

async fn get_top_games<'a>(client: &Client, offset: u16) -> Result<Vec<Cow<'a, str>>> {
    Compat::new(async {
        let resp = client
            .get(KRAKEN_TOP_GAMES)
            .query(&[("offset", offset), ("limit", 100)])
            .send()
            .await?;

        if resp.status() == StatusCode::BAD_REQUEST {
            let error = resp.json::<ErrorResponse>().await?;
            return Err(anyhow!("Could not get top games: {}", error.message));
        }

        let games = resp
            .error_for_status()?
            .json::<TopGamesResponse>()
            .await?
            .top
            .into_iter()
            .map(|game| game.game.name)
            .collect();

        Ok(games)
    })
    .await
}

async fn get_streams_page<'a>(
    client: &Client,
    game: &str,
    offset: u16,
) -> Result<Vec<Cow<'a, str>>> {
    Compat::new(async {
        let resp = client
            .get(KRAKEN_STREAMS)
            .query(&[("offset", offset), ("limit", 100)])
            .query(&[("game", game)])
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

async fn get_all_streams_for_game<'a>(client: &Client, game: String) -> Result<Vec<Cow<'a, str>>> {
    let mut futures = Vec::with_capacity(10);

    for i in 0..=9 {
        let offset = i * 100;
        futures.push(get_streams_page(&client, &game, offset));
    }

    let streams = try_join_all(futures)
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<Cow<'a, str>>>();

    info!("Found {} channels streaming {}", streams.len(), game);

    Ok(streams)
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

    let games = get_top_games(&client, 0).await?;

    info!("Found {} games", games.len());
    info!("Getting up to {} streams", 1000 * games.len());

    let mut futures = Vec::with_capacity(games.len());

    for game in games {
        futures.push(get_all_streams_for_game(&client, game.to_string()));
    }

    let streams = try_join_all(futures).await?.into_iter().flatten().collect();

    Ok(streams)
}

fn main() -> Result<()> {
    flexi_logger::Logger::with_env_or_str("info")
        .format(logger_format)
        .start()?;

    let mut channels = smol::block_on(get_streams())?;

    info!("Found {} channels currently streaming", channels.len());

    let mut config = Config::load()?;
    let old_count = config.channels.len();

    config.channels.append(&mut channels);
    config.channels.sort();
    config.channels.dedup();

    info!(
        "Saving {} new channels for a total of {}",
        config.channels.len() - old_count,
        config.channels.len()
    );

    config.save()?;

    Ok(())
}
