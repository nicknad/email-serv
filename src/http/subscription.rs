use axum::Router;
use axum::extract::Query;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::routing::{get, post};

use crate::http::ApiContext;
use crate::http::SubscriptionEmail;

#[derive(serde::Deserialize)]
pub struct SubscriptionRequest {
    pub email: String,
}

#[derive(serde::Deserialize)]
pub struct UnsubscribeParams {
    pub token: String,
}

fn hash_email(email: &str, key: &[u8; 32]) -> String {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(email.as_bytes());
    hasher.finalize().to_hex().to_string()
}

//Extractors always run in the order of the function parameters that is from left to right.
//
//The request body is an asynchronous stream that can only be consumed once. Therefore you can only have one extractor that consumes the request body. axum enforces this by requiring such extractors to be the last argument your handler takes.
pub async fn subscribe(
    State(context): State<ApiContext>,
    Json(body): Json<SubscriptionRequest>,
) -> (StatusCode, String) {
    if body.email.is_empty() {
        return (StatusCode::BAD_REQUEST, "Email is empty".to_string());
    }

    // Todo add email validation

    let email_hash = hash_email(&body.email, &context.blake3_key);
    let mut email_hashmap = context.emails.lock();
    if email_hashmap.is_empty() {
        email_hashmap.insert(
            email_hash,
            SubscriptionEmail {
                email: body.email,
                is_verified: false,
            },
        );

        return (StatusCode::OK, format!("Subscribed!"));
    }

    if email_hashmap.contains_key(&email_hash) {
        return (StatusCode::OK, format!("Subscribed!"));
    }

    email_hashmap.insert(
        email_hash,
        SubscriptionEmail {
            email: body.email,
            is_verified: false,
        },
    );

    (StatusCode::OK, format!("Subscribed!"))
}

pub async fn verify_subscription(
    State(context): State<ApiContext>,
    Query(params): Query<UnsubscribeParams>,
) -> StatusCode {
    if params.token.is_empty() {
        return StatusCode::BAD_REQUEST;
    }

    let mut email_hashmap = context.emails.lock();
    if let Some(x) = email_hashmap.get_mut(&params.token) {
        x.is_verified = true;
    }

    StatusCode::OK
}

pub async fn unsubscribe(
    State(context): State<ApiContext>,
    Query(params): Query<UnsubscribeParams>,
) -> StatusCode {
    if params.token.is_empty() {
        return StatusCode::BAD_REQUEST;
    }

    let mut emails = context.emails.lock();
    emails.remove(&params.token);

    StatusCode::OK
}

pub fn router() -> Router<ApiContext> {
    Router::new()
        .route("/api/subscribe", post(subscribe))
        .route("/api/verify", get(verify_subscription))
        .route("/api/unsubscribe", get(unsubscribe))
}
