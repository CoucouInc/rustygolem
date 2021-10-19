use anyhow::Context;
use async_trait::async_trait;
use chrono::Utc;
use diesel::serialize::ToSql;
use diesel::{backend::Backend, prelude::*, sql_types};
use diesel::{deserialize::FromSql, sql_types::Text};
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{multispace1, multispace0};
use nom::combinator::{all_consuming, map};
use nom::sequence::{preceded, terminated, tuple};
use nom::{Finish, IResult};
use reqwest::Client;
use serde::Deserialize;
use std::result::Result as StdResult;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task;

use crate::plugin::{Error, Plugin, Result};
use crate::schema::crypto_rate::{self, dsl};
// use crate::utils::messages::handle_errors;
use crate::utils::parser::{self, command_prefix};
// use crate::{db, utils::messages::with_target};
use super::db;
use irc::proto::{Command, Message};

pub struct Crypto {}

#[async_trait]
impl Plugin for Crypto {
    async fn init() -> Result<Self> {
        let _db_conn: Result<_> = tokio::task::spawn_blocking(|| {
            let conn = db::establish_connection()?;
            db::run_migrations(&conn)?;
            Ok(conn)
        })
        .await
        .map_err(|e| {
            let e: anyhow::Error = e.into();
            e
        })?;


        Ok(Crypto {})
    }

    fn get_name(&self) -> &'static str {
        "crypto"
    }

    async fn in_message(&self, msg: &Message) -> Result<Option<Message>> {
        in_msg(msg).await
    }

    async fn run(&self, _bot_chan: mpsc::Sender<Message>) -> Result<()> {
        monitor_crypto_coins().await?;
        Err(Error::Synthetic(
            "crypto coin monitoring job stopped".to_string(),
        ))
    }
}

async fn in_msg(msg: &Message) -> Result<Option<Message>> {
    let response_target = match msg.response_target() {
        None => return Ok(None),
        Some(target) => target.to_string(),
    };

    if let Command::PRIVMSG(_source, message) = &msg.command {
        let (mb_coin, mb_target) = match parse_command(message) {
            Ok(x) => x,
            Err(_) => return Ok(None),
        };
        let msg = match mb_coin {
            Ok(coin) => get_rate_and_history(coin).await?,
            Err(x) => {
                format!("Dénomination inconnue: {}. Ici on ne deal qu'avec des monnais vaguement respectueuses comme btc (aka xbt), eth, doge, xrp et algo.", x)
            }
        };
        let full_msg = crate::utils::messages::with_target(&msg, &mb_target);
        let irc_message = Command::PRIVMSG(response_target, full_msg).into();
        return Ok(Some(irc_message));
    }
    Ok(None)
}

fn parse_command(input: &str) -> StdResult<(StdResult<CryptoCoin, &str>, Option<&str>), String> {
    all_consuming(terminated(parse_crypto, multispace0))(input)
        .finish()
        .map(|x| x.1)
        .map_err(|e| format!("{:?}", e))
}

fn parse_crypto(input: &str) -> IResult<&str, (StdResult<CryptoCoin, &str>, Option<&str>)> {
    preceded(
        command_prefix,
        map(
            parser::with_target(tuple((tag("crypto"), multispace1, crypto_cmd))),
            |((_, _, c), t)| (c, t),
        ),
    )(input)
}

fn crypto_cmd(input: &str) -> IResult<&str, StdResult<CryptoCoin, &str>> {
    alt((
        map(tag("xbt"), |_| Ok(CryptoCoin::Bitcoin)),
        map(tag("btc"), |_| Ok(CryptoCoin::Bitcoin)),
        map(tag("eth"), |_| Ok(CryptoCoin::Ethereum)),
        map(tag("doge"), |_| Ok(CryptoCoin::Doge)),
        map(tag("xrp"), |_| Ok(CryptoCoin::Ripple)),
        map(tag("algo"), |_| Ok(CryptoCoin::Algorand)),
        map(parser::word, Err),
    ))(input)
}

#[derive(Debug, FromSqlRow, AsExpression, PartialEq, Clone, Copy)]
#[sql_type = "Text"]
enum CryptoCoin {
    Bitcoin,
    Ethereum,
    Doge,
    Ripple,
    Algorand,
}

impl<DB> FromSql<sql_types::Text, DB> for CryptoCoin
where
    DB: Backend,
    String: FromSql<sql_types::Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> diesel::deserialize::Result<Self> {
        match &(String::from_sql(bytes)?)[..] {
            "BTC" => Ok(CryptoCoin::Bitcoin),
            "ETH" => Ok(CryptoCoin::Ethereum),
            "DOGE" => Ok(CryptoCoin::Doge),
            "XRP" => Ok(CryptoCoin::Ripple),
            "ALGO" => Ok(CryptoCoin::Algorand),
            x => Err(format!("Unknown denomination: {}", x).into()),
        }
    }
}

impl<DB> ToSql<sql_types::Text, DB> for CryptoCoin
where
    DB: Backend,
{
    fn to_sql<W: std::io::Write>(
        &self,
        out: &mut diesel::serialize::Output<W, DB>,
    ) -> diesel::serialize::Result {
        let tag = match self {
            CryptoCoin::Bitcoin => "BTC",
            CryptoCoin::Ethereum => "ETH",
            CryptoCoin::Doge => "DOGE",
            CryptoCoin::Ripple => "XRP",
            CryptoCoin::Algorand => "ALGO",
        };
        ToSql::<sql_types::Text, DB>::to_sql(tag, out)
    }
}

impl std::fmt::Display for CryptoCoin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoCoin::Bitcoin => f.write_str("bitcoin"),
            CryptoCoin::Ethereum => f.write_str("ethereum"),
            CryptoCoin::Doge => f.write_str("dogecoin"),
            CryptoCoin::Ripple => f.write_str("ripple"),
            CryptoCoin::Algorand => f.write_str("algorand"),
        }
    }
}

// a bit tedious to map a rust struct from json
// which doesn't immediately reflect the structure.
// So use tmp structs and the serde_derive feature
#[derive(Debug, Deserialize, PartialEq)]
struct CryptowatchResponse {
    result: CryptowatchResponseResult,
    allowance: CryptowatchResponseAllowance,
}

#[derive(Debug, Deserialize, PartialEq)]
struct CryptowatchResponseResult {
    price: f32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct CryptowatchResponseAllowance {
    cost: f32,
    remaining: f32,
}

impl CryptoCoin {
    async fn get_rate_in_euro(&self, http_client: &Client) -> anyhow::Result<f32> {
        let symbol = match &self {
            CryptoCoin::Bitcoin => "btc",
            CryptoCoin::Ethereum => "eth",
            CryptoCoin::Doge => "doge",
            CryptoCoin::Ripple => "xrp",
            CryptoCoin::Algorand => "algo",
        };
        let exchange = match &self {
            CryptoCoin::Bitcoin => "bitstamp",
            CryptoCoin::Ethereum => "bitstamp",
            CryptoCoin::Doge => "bittrex",
            CryptoCoin::Ripple => "bittrex",
            CryptoCoin::Algorand => "coinbase-pro",
        };
        let url = format!(
            "https://api.cryptowat.ch/markets/{}/{}eur/price",
            exchange, symbol
        );

        let json_resp = http_client
            .get(&url)
            .send()
            .await?
            .json::<CryptowatchResponse>()
            .await
            .context(format!("Error while fetching response from {}", url))?;

        log::info!("cryptowatch response: {:?}", json_resp);
        Ok(json_resp.result.price)
    }
}

#[derive(Debug, Queryable, Insertable)]
#[table_name = "crypto_rate"]
struct CryptoCoinRate {
    date: chrono::NaiveDateTime,
    coin: CryptoCoin,
    rate: f32,
}

/// fetch, and save all crypto rates every minute
async fn monitor_crypto_coins() -> anyhow::Result<()> {
    loop {
        get_and_save_all_rates().await?;
        tokio::time::sleep(Duration::from_secs(60 * 60)).await;
    }
}

async fn get_and_save_all_rates() -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let (btc_rate, eth_rate, doge_rate, ripple_rate, algo_rate) = try_join!(
        CryptoCoin::Bitcoin.get_rate_in_euro(&client),
        CryptoCoin::Ethereum.get_rate_in_euro(&client),
        CryptoCoin::Doge.get_rate_in_euro(&client),
        CryptoCoin::Ripple.get_rate_in_euro(&client),
        CryptoCoin::Algorand.get_rate_in_euro(&client),
    )?;

    let btc_row = CryptoCoinRate {
        date: chrono::Utc::now().naive_utc(),
        coin: CryptoCoin::Bitcoin,
        rate: btc_rate,
    };

    let eth_row = CryptoCoinRate {
        date: chrono::Utc::now().naive_utc(),
        coin: CryptoCoin::Ethereum,
        rate: eth_rate,
    };

    let doge_row = CryptoCoinRate {
        date: chrono::Utc::now().naive_utc(),
        coin: CryptoCoin::Doge,
        rate: doge_rate,
    };

    let ripple_row = CryptoCoinRate {
        date: chrono::Utc::now().naive_utc(),
        coin: CryptoCoin::Ripple,
        rate: ripple_rate,
    };

    let algo_row = CryptoCoinRate {
        date: chrono::Utc::now().naive_utc(),
        coin: CryptoCoin::Algorand,
        rate: algo_rate,
    };

    task::spawn_blocking(move || {
        let conn = db::establish_connection()?;
        let vals = (&btc_row, &eth_row, &doge_row, &ripple_row, &algo_row);
        diesel::insert_into(crypto_rate::table)
            .values(vals)
            .execute(&conn)
            .with_context(|| format!("Cannot insert {:?} into db", vals))
    })
    .await??;

    Ok(())
}

// async fn handle_command(
//     cmd: StdResult<CryptoCoin, &str>,
//     mb_target: Option<&str>,
// ) -> Option<String> {
//     let message = match cmd {
//         Err(x) => {
//             format!("Dénomination inconnue: {}. Ici on ne deal qu'avec des monnais vaguement respectueuses comme btc (aka xbt), eth, doge, xrp et algo.", x)
//         }
//         Ok(c) => handle_errors(get_rate_and_history(c).await),
//     };
//     Some(with_target(&message, &mb_target))
// }

async fn get_rate_and_history(coin: CryptoCoin) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let rate = coin.get_rate_in_euro(&client).await?;
    let row = CryptoCoinRate {
        date: chrono::Utc::now().naive_utc(),
        coin,
        rate,
    };
    task::spawn_blocking(move || {
        let conn = db::establish_connection()?;
        diesel::insert_into(crypto_rate::table)
            .values(&row)
            .execute(&conn)
            .with_context(|| format!("Cannot insert {:?} into db", row))?;

        let now = Utc::now();
        let past_day = dsl::crypto_rate
            .filter(dsl::date.le((now - chrono::Duration::days(1)).naive_utc()))
            .filter(dsl::coin.eq(coin))
            .order_by(dsl::date.desc())
            .limit(1)
            .load::<CryptoCoinRate>(&conn)?
            .into_iter()
            .next();

        let past_week = dsl::crypto_rate
            .filter(dsl::date.le((now - chrono::Duration::days(7)).naive_utc()))
            .filter(dsl::coin.eq(coin))
            .order_by(dsl::date.desc())
            .limit(1)
            .load::<CryptoCoinRate>(&conn)?
            .into_iter()
            .next();

        let past_month = dsl::crypto_rate
            // not quite 1 month, but 🤷
            .filter(dsl::date.le((now - chrono::Duration::days(30)).naive_utc()))
            .filter(dsl::coin.eq(coin))
            .order_by(dsl::date.desc())
            .limit(1)
            .load::<CryptoCoinRate>(&conn)?
            .into_iter()
            .next();

        log::debug!(
            "current rate: {}, past day: {:?}, past week: {:?}, past month: {:?}",
            rate,
            past_day,
            past_week,
            past_month
        );

        let variations = vec![(past_day, "1D"), (past_week, "1W"), (past_month, "1M")]
            .into_iter()
            .filter_map(|(mb_r, suffix)| {
                mb_r.map(|r| {
                    let var = RateVariation(((rate - r.rate) * 100.0) / r.rate);
                    format!("{:.02} {}", var, suffix)
                })
            })
            .collect::<Vec<_>>();

        let variations = if variations.is_empty() {
            "".to_string()
        } else {
            format!("({})", variations.join(" − "))
        };

        let result = format!(
            "1 {} vaut {} euros grâce au pouvoir de la spéculation ! {}",
            coin, rate, variations
        );

        Ok(result)
    })
    .await?
}

struct RateVariation(f32);

impl std::fmt::Display for RateVariation {
    fn fmt(&self, mut f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let r = self.0;
        // (↘0.97% 1D − ↗24.25% 1W − ↗43.32% 1M)

        match r.partial_cmp(&0.) {
            Some(std::cmp::Ordering::Less) => f.write_str("↘")?,
            Some(std::cmp::Ordering::Greater) => f.write_str("↗")?,
            _ => f.write_str("−")?,
        }
        r.abs().fmt(&mut f)?;
        f.write_str("%")?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    async fn price_from_json() {
        let json = r#"{"result":{"price":30250.14},"allowance":{"cost":0.005,"remaining":9.98,"upgrade":"For unlimited API access, create an account at https://cryptowat.ch"}}"#;
        let expected = CryptowatchResponse {
            result: CryptowatchResponseResult { price: 30250.14 },
            allowance: CryptowatchResponseAllowance {
                cost: 0.005,
                remaining: 9.98,
            },
        };

        assert_eq!(
            serde_json::from_str(json).map_err(|e| format!("{:?}", e)),
            // CryptowatchPrice::from_str(json).map_err(|e| format!("{:?}", e)),
            Ok(expected)
        )
    }

    #[test]
    async fn test_crypto() {
        assert!(
            parse_command("λcrypto").is_err(),
            "must have something after the command"
        );

        assert_eq!(
            parse_command("λcrypto xbt"),
            Ok((Ok(CryptoCoin::Bitcoin), None)),
            "can parse bitcoin"
        );

        assert_eq!(
            parse_command("λcrypto wut"),
            Ok((Err("wut"), None)),
            "inner error on unknown coin"
        );

    }
}
