use anyhow::{Context, Result};
use chrono::{Datelike, Utc};
use diesel::serialize::ToSql;
use diesel::{backend::Backend, prelude::*, sql_types};
use diesel::{deserialize::FromSql, sql_types::Text};
use reqwest::Client;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task;

use crate::schema::crypto_rate::{self, dsl};
use crate::utils::messages::handle_errors;
use crate::{db, utils::messages::with_target};

#[derive(Debug, FromSqlRow, AsExpression, PartialEq)]
#[sql_type = "Text"]
pub enum CryptoCoin {
    Bitcoin,
    Ethereum,
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
        };
        ToSql::<sql_types::Text, DB>::to_sql(tag, out)
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
    async fn get_rate_in_euro(&self, http_client: &Client) -> Result<f32> {
        let symbol = match &self {
            CryptoCoin::Bitcoin => "btc",
            CryptoCoin::Ethereum => "eth",
        };
        let url = format!(
            "https://api.cryptowat.ch/markets/bitstamp/{}eur/price",
            symbol
        );

        let json_resp = http_client
            .get(&url)
            .send()
            .await?
            .json::<CryptowatchResponse>()
            .await
            .context(format!("Error while fetching response from {}", url))?;

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

// pub fn test() -> Result<()> {
//     Ok(())
// }

/// fetch, and save all crypto rates every minute
pub async fn monitor_crypto_coins() -> Result<()> {
    loop {
        get_and_save_all_rates().await?;
        tokio::time::sleep(Duration::from_secs(60 * 60)).await;
    }
}

pub async fn get_and_save_all_rates() -> Result<()> {
    let client = reqwest::Client::new();
    let (btc_rate, eth_rate) = try_join!(
        CryptoCoin::Bitcoin.get_rate_in_euro(&client),
        CryptoCoin::Ethereum.get_rate_in_euro(&client),
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

    task::spawn_blocking(move || {
        let conn = db::establish_connection()?;
        diesel::insert_into(crypto_rate::table)
            .values((&btc_row, &eth_row))
            .execute(&conn)
            .with_context(|| format!("Cannot insert {:?} into db", (&btc_row, &eth_row)))
    })
    .await??;

    Ok(())
}

pub(crate) async fn handle_command(
    cmd: std::result::Result<CryptoCoin, &str>,
    mb_target: Option<&str>,
) -> Option<String> {
    let message = match cmd {
        Err(x) => {
            format!("Dénomination inconnue: {}. Ici on ne deal qu'avec des monnais respectueuses comme btc (aka xbt) et eth.", x)
        }
        Ok(c) => handle_errors(get_rate_and_history(c).await),
    };
    Some(with_target(&message, &mb_target))
}

async fn get_rate_and_history(coin: CryptoCoin) -> Result<String> {
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
            .order_by(dsl::date.desc())
            .limit(1)
            .load::<CryptoCoinRate>(&conn)?
            .into_iter()
            .next();

        let past_week = dsl::crypto_rate
            .filter(dsl::date.le((now - chrono::Duration::days(7)).naive_utc()))
            .order_by(dsl::date.desc())
            .limit(1)
            .load::<CryptoCoinRate>(&conn)?
            .into_iter()
            .next();

        let past_month = dsl::crypto_rate
            // not quite 1 month, but 🤷
            .filter(dsl::date.le((now - chrono::Duration::days(30)).naive_utc()))
            .order_by(dsl::date.desc())
            .limit(1)
            .load::<CryptoCoinRate>(&conn)?
            .into_iter()
            .next();

        let result = vec![(past_day, "1D"), (past_week, "1W"), (past_month, "1M")]
            .into_iter()
            .filter_map(|(mb_r, suffix)| {
                mb_r.map(|r| {
                    let var = RateVariation((rate - r.rate) / rate * 100.0);
                    format!("{:.02} {}", var, suffix)
                })
            })
            .collect::<Vec<_>>();

        let x: Result<_> = Ok(result.join(" − "));
        x
    })
    .await?
}

struct RateVariation(f32);

impl std::fmt::Display for RateVariation {
    fn fmt(&self, mut f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let r = self.0;

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
    use serde_json;

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
}
