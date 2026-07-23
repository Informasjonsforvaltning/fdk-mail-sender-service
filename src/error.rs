use actix_web::{HttpResponse, ResponseError};

use crate::models;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    String(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),
    #[error(transparent)]
    LettreTransportError(#[from] lettre::transport::smtp::Error),
    #[error(transparent)]
    LettreError(#[from] lettre::error::Error),
    #[error(transparent)]
    AddressError(#[from] lettre::address::AddressError),
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Self::String(e.to_string())
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::String(e)
    }
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        use Error::*;

        match self {
            LettreTransportError(e) => {
                tracing::error!(error = e.to_string().as_str(), "Unable to send mail")
            }
            e => {
                tracing::warn!(
                    error = e.to_string().as_str(),
                    "Error occured when handling request"
                )
            }
        };

        match self {
            Unauthorized(_) => HttpResponse::Unauthorized().json(models::Error::message(self)),
            _ => HttpResponse::InternalServerError().json(models::Error::error(self)),
        }
    }
}

impl models::Error {
    fn message<S: ToString>(message: S) -> Self {
        models::Error {
            message: Some(message.to_string()),
            ..Default::default()
        }
    }
    fn error<S: ToString>(error: S) -> Self {
        models::Error {
            error: Some(error.to_string()),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use actix_web::{body::to_bytes, http::StatusCode, ResponseError};

    use super::*;

    async fn response_json(err: &Error) -> (StatusCode, models::Error) {
        let response = err.error_response();
        let status = response.status();
        let body = to_bytes(response.into_body()).await.unwrap();
        let json: models::Error = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    #[actix_web::test]
    async fn unauthorized_returns_401_with_message() {
        let err = Error::Unauthorized("X-API-KEY header missing".to_string());
        let (status, json) = response_json(&err).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(json.message.is_some());
        assert!(json.message.unwrap().contains("Unauthorized"));
        assert!(json.error.is_none());
    }

    #[actix_web::test]
    async fn string_error_returns_500_with_error_field() {
        let err = Error::String("boom".to_string());
        let (status, json) = response_json(&err).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json.error.as_deref(), Some("boom"));
        assert!(json.message.is_none());
    }

    #[actix_web::test]
    async fn serde_json_error_returns_500_with_error_field() {
        let serde_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = Error::from(serde_err);
        let (status, json) = response_json(&err).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(json.error.is_some());
        assert!(json.message.is_none());
    }
}
