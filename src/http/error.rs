use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(thiserror::Error, Debug)]
pub enum EmailServError {
    #[error("Failed to create log directory: {0}")]
    LogDirectoryCreationFailed(#[from] std::io::Error),

    #[error("Email address cannot be empty")]
    EmailEmpty,

    #[error("Invalid email format: {0}")]
    EmailInvalid(String),

    #[error("Token cannot be empty")]
    TokenEmpty,

    #[error("Subscription not found for token: {0}")]
    SubscriptionNotFound(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),

    #[error("Template error: {0}")]
    TemplateError(String),

    #[error("Email delivery failed: {0}")]
    EmailDeliveryFailed(String),

    #[error("Unauthorized: missing or invalid admin API key")]
    Unauthorized,

    #[error("Bad request: {0}")]
    BadRequest(String),
}

impl EmailServError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::LogDirectoryCreationFailed(_)
            | Self::DatabaseError(_)
            | Self::TemplateError(_)
            | Self::EmailDeliveryFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,

            Self::EmailEmpty
            | Self::EmailInvalid(_)
            | Self::TokenEmpty
            | Self::SubscriptionNotFound(_)
            | Self::BadRequest(_) => StatusCode::BAD_REQUEST,

            Self::Unauthorized => StatusCode::UNAUTHORIZED,
        }
    }
}

impl IntoResponse for EmailServError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = serde_json::json!({
            "error": self.to_string(),
            "code": status.as_u16()
        });

        (status, Json(body)).into_response()
    }
}
