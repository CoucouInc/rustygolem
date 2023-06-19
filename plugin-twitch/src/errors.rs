use rocket::http::Status;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TwitchSigError {
    #[error("Missing header {0}")]
    Missing(&'static str),
    #[error("Invalid signature")]
    Invalid,
    #[error("Missing env var for app secret")]
    MissingAppSecret(#[from] std::env::VarError),
}

#[derive(Error, Debug)]
pub enum TwitchError {
    #[error("Invalid signature {0:?}")]
    InvalidSig(#[from] TwitchSigError),

    #[error("RocketError")]
    RocketError(Status),
}

impl<'r> rocket::response::Responder<'r, 'static> for TwitchError {
    fn respond_to(self, _: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        let status = match &self {
            TwitchError::InvalidSig(_) => Status::Forbidden,
            TwitchError::RocketError(s) => *s,
        };
        let err_str = self.to_string();
        rocket::response::Response::build()
            .sized_body(err_str.len(), std::io::Cursor::new(err_str))
            .status(status)
            .header(rocket::http::ContentType::Text)
            .ok()
    }
}

pub type Result<T> = std::result::Result<T, TwitchError>;
