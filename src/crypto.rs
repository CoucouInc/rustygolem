use anyhow::{Context, Result};
use diesel::serialize::ToSql;
use diesel::{backend::Backend, prelude::*, sql_types};
use diesel::{deserialize::FromSql, sql_types::Text};
use reqwest::Client;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::task;
use std::time::Duration;

use crate::db;
use crate::schema::crypto_rate::{self, dsl::*};

pub async fn watch_rates() -> Result<()> {
    todo!()
}

#[derive(Debug, FromSqlRow, AsExpression)]
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
    // Self: ToSql<sql_types::Text, DB>,
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

pub fn test() -> Result<()> {
    let conn = db::establish_connection()?;
    let results = crypto_rate.limit(5).load::<CryptoCoinRate>(&conn)?;
    for r in results {
        println!("{:?}", r);
    }

    let d = chrono::Utc::now().naive_utc();
    let new_rate = CryptoCoinRate {
        date: d,
        coin: CryptoCoin::Ethereum,
        rate: 123.45,
    };

    diesel::insert_into(crypto_rate::table)
        .values(&new_rate)
        .execute(&conn)?;

    let r = crypto_rate
        .filter(crypto_rate::date.eq(d))
        .first::<CryptoCoinRate>(&conn)?;
    println!("{:#?}", r);

    Ok(())
}

/// fetch, and save all crypto rates every minute
pub async fn monitor_crypto_coins() -> Result<()> {
    loop {
        get_and_save_all_rates().await?;
        tokio::time::sleep(Duration::from_secs(60 * 60)).await;
    }
}

pub async fn get_and_save_all_rates() -> Result<()> {
    let client = reqwest::Client::new();
    let conn = task::spawn_blocking(db::establish_connection).await??;
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
        diesel::insert_into(crypto_rate::table)
            .values((&btc_row, &eth_row))
            .execute(&conn)
    })
    .await??;

    Ok(())
}

struct TestInsert {
    coucou: String,
}

pub async fn get_and_save_rate(
    client: &reqwest::Client,
    conn: Arc<Mutex<SqliteConnection>>,
    crypto_coin: CryptoCoin,
) -> Result<()> {
    let coin_rate = crypto_coin.get_rate_in_euro(&client).await?;

    task::spawn_blocking(move || {
        let row = CryptoCoinRate {
            date: chrono::Utc::now().naive_utc(),
            coin: crypto_coin,
            rate: coin_rate,
        };
        let conn = conn.lock().unwrap();
        diesel::insert_into(crypto_rate::table)
            .values(&row)
            .execute(&*conn)
    })
    .await??;
    Ok(())
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
