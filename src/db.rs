use anyhow::{Context, Result};
use diesel::prelude::*;
use diesel::Connection;
use reqwest::Client;
diesel_migrations::embed_migrations!("./migrations/");
use serde_json::{Map, Number, Value};

pub fn establish_connection() -> Result<SqliteConnection> {
    // let db_url = "rustycoucou.sqlite";
    let db_url = "lambdacoucou.sqlite";
    SqliteConnection::establish(&db_url).context(format!("cannot connect to db at {}", db_url))
}

pub fn run_migrations(connection: &SqliteConnection) -> Result<()> {
    embedded_migrations::run(connection)
        .context("Cannot run migration")
        .map_err(|e| e.into())
}

pub async fn watch_rates() -> Result<()> {
    todo!()
}

// #[derive(Debug)]
// enum CryptoCoin {
//     Bitcoin,
//     Ethereum,
// }
//
// #[derive(Debug, PartialEq)]
// pub struct CryptowatchPrice {
//     price: f64,
//     api_cost: f64,
//     remaining: f64,
// }
//
// fn expect_object<'val>(tag: &str, v: &'val serde_json::Value) -> Result<&'val Map<String, Value>> {
//     match v {
//         Value::Object(o) => Ok(o),
//         stuff => Err(anyhow!("{} must be an object but got {}", tag, stuff)),
//     }
// }
//
// fn expect_number<'val>(tag: &str, v: &'val serde_json::Value) -> Result<&'val Number> {
//     match v {
//         Value::Number(n) => Ok(n),
//         stuff => Err(anyhow!("{} must be a number but got {}", tag, stuff)),
//     }
// }
//
// // manual json deserialization for "fun"
// impl CryptowatchPrice {
//     fn from_str<'a>(s: &'a str) -> Result<Self> {
//         let val: serde_json::Value = serde_json::de::from_str(s)?;
//         let obj_val = match val {
//             serde_json::Value::Object(o) => Ok(o),
//             typ => Err(anyhow!("Expected toplevel object but got {}", typ)),
//         }?;
//
//         let result = expect_object(
//             "result",
//             obj_val
//                 .get("result")
//                 .ok_or(anyhow!("key 'result' not found"))?,
//         )?;
//         let allowance = expect_object(
//             "allowance",
//             obj_val
//                 .get("allowance")
//                 .ok_or(anyhow!("key 'allowance' not found"))?,
//         )?;
//
//         let price = expect_number(
//             "price",
//             result
//                 .get("price")
//                 .ok_or(anyhow!("key 'price' not found in result"))?,
//         )?;
//
//         let cost = expect_number(
//             "cost",
//             allowance
//                 .get("cost")
//                 .ok_or(anyhow!("key 'cost' not found in allowance"))?,
//         )?;
//
//         let remaining = expect_number(
//             "remaining",
//             allowance
//                 .get("remaining")
//                 .ok_or(anyhow!("key 'remaining' not found in allowance"))?,
//         )?;
//
//         Ok(CryptowatchPrice {
//             price: price.as_f64().ok_or(anyhow!("{} is not an f64", price))?,
//             api_cost: cost.as_f64().ok_or(anyhow!("{} is not an f64", cost))?,
//             remaining: remaining
//                 .as_f64()
//                 .ok_or(anyhow!("{} is not an f64", remaining))?,
//         })
//     }
// }
//
// impl CryptoCoin {
//     async fn get_rate_in_euro(&self, http_client: &Client) -> Result<f64> {
//         let symbol = match &self {
//             CryptoCoin::Bitcoin => "btc",
//             CryptoCoin::Ethereum => "eth",
//         };
//         let url = format!(
//             "https://api.cryptowat.ch/markets/bitstamp/{}eur/price",
//             symbol
//         );
//         let text_resp = http_client
//             .get(&url)
//             .send()
//             .await?
//             .text()
//             .await
//             .context(format!("Error while fetching response from {}", url))?;
//         println!("text resp for {}:\n{}", url, text_resp);
//         let resp = CryptowatchPrice::from_str(&text_resp)
//             .context(format!("Cannot parse CryptowatchPrice from: {}", text_resp))?;
//         println!("Remaining cryptowatch allowance is: {}", resp.remaining);
//         Ok(resp.price)
//     }
// }
//
// pub async fn get_and_save_rates() -> Result<()> {
//     let client = reqwest::Client::new();
//     let btc_price = CryptoCoin::Bitcoin.get_rate_in_euro(&client).await?;
//     let eth_price = CryptoCoin::Ethereum.get_rate_in_euro(&client).await?;
//     println!(
//         "1 btc vaut {}€ grace au pouvoir de la spéculation!",
//         btc_price
//     );
//     println!(
//         "1 eth vaut {}€ grace au pouvoir de la spéculation!",
//         eth_price
//     );
//     Ok(())
// }
//
// #[cfg(test)]
// mod test {
//     use super::*;
//     use pretty_assertions::assert_eq;
//
//     #[test]
//     fn price_from_json() {
//         let json = r#"{"result":{"price":30250.14},"allowance":{"cost":0.005,"remaining":9.98,"upgrade":"For unlimited API access, create an account at https://cryptowat.ch"}}"#;
//         let expected = CryptowatchPrice {
//             price: 30250.14,
//             api_cost: 0.005,
//             remaining: 9.98,
//         };
//
//         assert_eq!(
//             CryptowatchPrice::from_str(json).map_err(|e| format!("{:?}", e)),
//             Ok(expected)
//         )
//     }
// }
