use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CrabClawError {
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("network error: {0}")]
    Network(String),
    #[error("auth error: {0}")]
    Auth(String),
    #[error("api error: {0}")]
    Api(String),
    #[error("rate limited: {0}")]
    RateLimit(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, CrabClawError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_display() {
        let err = CrabClawError::Config("missing key".to_string());
        assert_eq!(err.to_string(), "config error: missing key");
    }

    #[test]
    fn network_error_display() {
        let err = CrabClawError::Network("timeout".to_string());
        assert_eq!(err.to_string(), "network error: timeout");
    }

    #[test]
    fn auth_error_display() {
        let err = CrabClawError::Auth("invalid token".to_string());
        assert_eq!(err.to_string(), "auth error: invalid token");
    }

    #[test]
    fn api_error_display() {
        let err = CrabClawError::Api("rate limited".to_string());
        assert_eq!(err.to_string(), "api error: rate limited");
    }

    #[test]
    fn io_error_from_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let err: CrabClawError = io_err.into();
        match err {
            CrabClawError::Io(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            other => panic!("expected Io, got: {other}"),
        }
    }
}
