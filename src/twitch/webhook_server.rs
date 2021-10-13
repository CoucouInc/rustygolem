use crate::twitch::errors::{self, TwitchError};
use anyhow::{anyhow, Context};
use hmac::{Hmac, Mac, NewMac};
use rocket::{config::Shutdown, request::Outcome, State};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    num::ParseIntError,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use twitch_api2::eventsub;

use crate::twitch::message::Message;

type HmacSha256 = Hmac<sha2::Sha256>;

fn decode_hex(s: &str) -> std::result::Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}

struct SigVerifier<'r> {
    expected_sig: Vec<u8>,
    msg_id: &'r str,
    msg_ts: &'r str,
}

impl<'r> SigVerifier<'r> {
    fn verify(&self, body: &[u8]) -> std::result::Result<(), errors::TwitchSigError> {
        let sub_secret = std::env::var("TWITCH_APP_SECRET")?;
        let mut mac = HmacSha256::new_from_slice(sub_secret.as_bytes()).unwrap();
        mac.update(self.msg_id.as_bytes());
        mac.update(self.msg_ts.as_bytes());
        mac.update(body);

        mac.verify(&self.expected_sig[..]).map_err(|_| {
            eprintln!("Signature verification failed!");
            errors::TwitchSigError::Invalid
        })?;
        Ok(())
    }
}

#[rocket::async_trait]
impl<'r> rocket::request::FromRequest<'r> for SigVerifier<'r> {
    type Error = errors::TwitchSigError;

    async fn from_request(req: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        let sig = match req.headers().get_one("Twitch-Eventsub-Message-Signature") {
            None => {
                return Outcome::Failure((
                    rocket::http::Status::BadRequest,
                    errors::TwitchSigError::Missing("message signature"),
                ))
            }
            Some(sig) => sig,
        };
        let sig = match decode_hex(&sig["sha256=".len()..]) {
            Ok(bs) => bs,
            Err(_) => {
                return Outcome::Failure((
                    rocket::http::Status::BadRequest,
                    errors::TwitchSigError::Invalid,
                ))
            }
        };

        let msg_id = match req.headers().get_one("Twitch-Eventsub-Message-Id") {
            None => {
                return Outcome::Failure((
                    rocket::http::Status::BadRequest,
                    errors::TwitchSigError::Missing("message id"),
                ))
            }
            Some(mid) => mid,
        };

        let msg_ts = match req.headers().get_one("Twitch-Eventsub-Message-Timestamp") {
            None => {
                return Outcome::Failure((
                    rocket::http::Status::BadRequest,
                    errors::TwitchSigError::Missing("message timestamp"),
                ))
            }
            Some(ts) => ts,
        };

        Outcome::Success(SigVerifier {
            expected_sig: sig,
            msg_id,
            msg_ts,
        })
    }
}

#[rocket::post("/touitche/coucou", data = "<input>")]
async fn webhook_post<'r>(
    input: &'r str,
    sig_verifier: SigVerifier<'_>,
    st: &State<ServerState>,
) -> errors::Result<String> {
    log::debug!("got something from twitch: {:#?}", input);
    sig_verifier.verify(input.as_bytes()).map_err(|err| {
        log::error!("Twitch signature verification failed! {:?}", err);
        errors::TwitchError::InvalidSig(err)
    })?;

    let payload = twitch_api2::eventsub::Payload::parse(input).expect("good twitch response");
    // dbg!(&payload);
    match payload {
        eventsub::Payload::VerificationRequest(verif_req) => {
            log::info!("verification request received: {:#?}", verif_req);
            Ok(verif_req.challenge)
        }
        eventsub::Payload::StreamOnlineV1(online) => {
            log::debug!("online stream event: {:#?}", online);
            st.send_chan
                .send(Message::StreamOnline(online.event))
                .await
                .map_err(|err| {
                    log::error!("{:?}", err);
                    TwitchError::RocketError(rocket::http::Status::InternalServerError)
                })?;
            Ok("".to_string())
        }
        eventsub::Payload::StreamOfflineV1(offline) => {
            log::debug!("offline stream event: {:#?}", offline);
            st.send_chan
                .send(Message::StreamOffline(offline.event))
                .await
                .map_err(|err| {
                    log::error!("{:?}", err);
                    TwitchError::RocketError(rocket::http::Status::InternalServerError)
                })?;
            Ok("".to_string())
        }
        _ => {
            log::info!("Received unsupported payload: {:#?}", payload);
            Ok("".to_string())
        }
    }
}

pub struct ServerState {
    send_chan: mpsc::Sender<Message>,
}

// TODO pass the config as argument, or read from a given Path
pub async fn run_server(tx: mpsc::Sender<Message>) -> anyhow::Result<()> {
    let config = rocket::Config {
        address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        port: 7777,
        shutdown: Shutdown {
            // let the tokio runtime handle termination
            // this effectively disable the grace period in rocket
            // for this usecase it's fine
            ctrlc: false,
            ..Shutdown::default()
        },
        ..rocket::Config::default()
    };

    let server_state = ServerState { send_chan: tx };

    let result = rocket::build()
        .mount("/", rocket::routes![webhook_post])
        .configure(config)
        .manage(server_state)
        .ignite()
        .await?
        .launch()
        .await;
    println!("The webhook server shutdown {:?}", result);
    Err(anyhow!("webhook server shut down"))
}
