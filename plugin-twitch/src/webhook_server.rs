use crate::errors::{self, TwitchError, TwitchSigError};
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, status::StatusCode},
    response::IntoResponse,
    routing, Router,
};
use hmac::{Hmac, Mac, NewMac};
use std::{num::ParseIntError, sync::Arc};
use tokio::sync::mpsc;
use twitch_api2::eventsub;

use crate::config::{Config, Message};

type HmacSha256 = Hmac<sha2::Sha256>;

fn decode_hex(s: &str) -> std::result::Result<Vec<u8>, ParseIntError> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect()
}

struct SigVerifierAxum {
    expected_sig: Vec<u8>,
    msg_id: Vec<u8>,
    msg_ts: Vec<u8>,
}

impl SigVerifierAxum {
    fn verify(&self, sub_secret: &str, body: &[u8]) -> Result<(), TwitchSigError> {
        let mut mac = HmacSha256::new_from_slice(sub_secret.as_bytes()).unwrap();
        mac.update(&self.msg_id);
        mac.update(&self.msg_ts);
        mac.update(body);

        mac.verify(&self.expected_sig[..]).map_err(|_| {
            eprintln!("Signature verification failed!");
            errors::TwitchSigError::Invalid
        })?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<S> FromRequestParts<S> for SigVerifierAxum
where
    S: Send + Sync,
{
    type Rejection = TwitchSigError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection>
    where
        S: Send + Sync,
    {
        let sig = match parts.headers.get("Twitch-Eventsub-Message-Signature") {
            Some(sig) => match sig.to_str() {
                Ok(sig) => sig,
                Err(_err) => return Err(TwitchSigError::Invalid),
            },
            None => return Err(TwitchSigError::Missing("message signature")),
        };

        let sig = match decode_hex(&sig["sha256=".len()..]) {
            Ok(bs) => bs,
            Err(_) => return Err(TwitchSigError::Invalid),
        };

        let msg_id = match parts.headers.get("Twitch-Eventsub-Message-Id") {
            Some(hdr) => hdr.as_bytes().to_vec(),
            None => return Err(TwitchSigError::Missing("message id")),
        };

        let msg_ts = match parts.headers.get("Twitch-Eventsub-Message-Timestamp") {
            Some(hdr) => hdr.as_bytes().to_vec(),
            None => return Err(TwitchSigError::Missing("message timestamp")),
        };

        Ok(SigVerifierAxum {
            expected_sig: sig,
            msg_id,
            msg_ts,
        })
    }
}

#[derive(Clone)]
pub struct ServerStateAxum {
    app_secret: Arc<String>,
    send_chan: mpsc::Sender<Message>,
}

async fn webhook_post2(
    sig_verifier: SigVerifierAxum,
    axum::extract::State(state): axum::extract::State<ServerStateAxum>,
    body: String,
) -> Result<axum::response::Response, TwitchError> {
    log::debug!("got something from twitch: {:?}", body);
    sig_verifier.verify(&state.app_secret, body.as_bytes())?;

    let payload = twitch_api2::eventsub::Payload::parse(&body).expect("good twitch response");
    // dbg!(&payload);
    match payload {
        eventsub::Payload::VerificationRequest(verif_req) => {
            log::debug!("verification request received: {:#?}", verif_req);
            Ok(verif_req.challenge.into_response())
        }
        eventsub::Payload::StreamOnlineV1(online) => {
            log::debug!("online stream event: {:#?}", online);
            state
                .send_chan
                .send(Message::StreamOnline(online.event))
                .await
                .map_err(|err| {
                    log::error!("{:?}", err);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(().into_response())
        }
        eventsub::Payload::StreamOfflineV1(offline) => {
            log::debug!("offline stream event: {:#?}", offline);
            state
                .send_chan
                .send(Message::StreamOffline(offline.event))
                .await
                .map_err(|err| {
                    log::error!("{:?}", err);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            Ok(().into_response())
        }
        _ => {
            log::info!("Received unsupported payload: {:#?}", payload);
            Err(StatusCode::NOT_IMPLEMENTED.into())
        }
    }
}

pub(crate) fn init_router(config: &Config, tx: mpsc::Sender<Message>) -> Router<()> {
    let server_state = ServerStateAxum {
        app_secret: Arc::new(config.app_secret.clone()),
        send_chan: tx,
    };

    axum::Router::new()
        .route("/touitche/coucou", routing::post(webhook_post2))
        .with_state(server_state.clone())
}
