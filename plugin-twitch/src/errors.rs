use axum::{
    http::status::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TwitchSigError {
    #[error("Missing header {0}")]
    Missing(&'static str),
    #[error("Invalid signature")]
    Invalid,
    #[error("Invalid header value")]
    InvalidHeader(#[from] axum::http::header::ToStrError),
    #[error("Missing env var for app secret")]
    MissingAppSecret(#[from] std::env::VarError),
}

#[derive(Error, Debug)]
pub enum TwitchError {
    #[error("Invalid signature {0:?}")]
    InvalidSig(#[from] TwitchSigError),

    #[error("HttpError {0}")]
    HttpError(StatusCode),
}

impl std::convert::From<StatusCode> for TwitchError {
    fn from(value: StatusCode) -> Self {
        TwitchError::HttpError(value)
    }
}

impl IntoResponse for TwitchSigError {
    fn into_response(self) -> Response {
        println!("twitchsigerror to response: {self:?}");
        match self {
            e@TwitchSigError::Missing(_) => {
                (StatusCode::BAD_REQUEST, format!("{e}")).into_response()
            }
            TwitchSigError::Invalid => {
                (StatusCode::BAD_REQUEST, "invalid signature").into_response()
            }
            e@TwitchSigError::InvalidHeader(_) => {
                (StatusCode::BAD_REQUEST, format!("{e}")).into_response()
            }
            TwitchSigError::MissingAppSecret(e) => {
                log::error!("{e:?}");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

impl IntoResponse for TwitchError {
    fn into_response(self) -> Response {
        match self {
            e@TwitchError::InvalidSig(_) => {
                (StatusCode::BAD_REQUEST, format!("{e}")).into_response()
            }
            TwitchError::HttpError(code) => code.into_response(),
        }
    }
}
