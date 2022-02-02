use crate::plugins::twitch::errors::{self, TwitchError};
use anyhow::Context;
use hmac::{Hmac, Mac, NewMac};
use rocket::{config::Shutdown, request::Outcome, State};
use std::num::ParseIntError;
use tokio::sync::mpsc;
use twitch_api2::eventsub;

use crate::plugin::{self, Error};
use crate::plugins::twitch::config::{Config, Message};

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
    fn verify(
        &self,
        sub_secret: &str,
        body: &[u8],
    ) -> std::result::Result<(), errors::TwitchSigError> {
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
    input: &str,
    sig_verifier: SigVerifier<'_>,
    st: &State<ServerState>,
) -> errors::Result<String> {
    log::debug!("got something from twitch: {:#?}", input);
    sig_verifier
        .verify(&st.app_secret, input.as_bytes())
        .map_err(|err| {
            log::error!("Twitch signature verification failed! {:?}", err);
            errors::TwitchError::InvalidSig(err)
        })?;

    let payload = twitch_api2::eventsub::Payload::parse(input).expect("good twitch response");
    // dbg!(&payload);
    match payload {
        eventsub::Payload::VerificationRequest(verif_req) => {
            log::debug!("verification request received: {:#?}", verif_req);
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
    app_secret: String,
    send_chan: mpsc::Sender<Message>,
}

pub async fn run(config: &Config, tx: mpsc::Sender<Message>) -> plugin::Result<()> {
    let bind = &config.webhook_bind;
    let rocket_config = rocket::Config {
        address: bind
            .parse()
            .with_context(|| format!("Cannot parse {} as ipv4 or ipv6", bind))?,
        port: config.webhook_port,
        shutdown: Shutdown {
            // let the tokio runtime handle termination
            // this effectively disable the grace period in rocket
            // for this usecase it's fine
            ctrlc: false,
            ..Shutdown::default()
        },
        ..rocket::Config::default()
    };

    let server_state = ServerState {
        app_secret: config.app_secret.clone(),
        send_chan: tx,
    };

    let result = rocket::build()
        .mount("/", rocket::routes![webhook_post])
        .configure(rocket_config)
        .manage(server_state)
        .ignite()
        .await
        .context("Cannot ignite rocket")?
        .launch()
        .await;
    log::error!("The webhook server shutdown {:?}", result);
    Err(Error::Synthetic("twitch webhook server shut down".to_string()))
}
