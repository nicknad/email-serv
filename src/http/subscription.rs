use axum::Router;
use axum::extract::Query;
use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};

use crate::http::ApiContext;
use crate::http::error::EmailServError;

#[derive(serde::Deserialize)]
pub struct SubscriptionRequest {
    pub email: String,
}

#[derive(serde::Deserialize)]
pub struct TokenParams {
    pub token: String,
}

pub fn hash_email(email: &str, key: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(key);
    hasher.update(email.as_bytes());
    *hasher.finalize().as_bytes()
}

fn decode_token(token: &str) -> Result<[u8; 32], EmailServError> {
    if token.is_empty() {
        return Err(EmailServError::TokenEmpty);
    }

    let mut bytes = [0u8; 32];
    hex::decode_to_slice(token, &mut bytes)
        .map_err(|_| EmailServError::SubscriptionNotFound(token.to_string()))?;
    Ok(bytes)
}

fn validate_email(email: &str) -> Result<(), EmailServError> {
    if email.is_empty() {
        return Err(EmailServError::EmailEmpty);
    }

    if !email.contains('@') || email.len() < 5 || email.len() > 254 {
        return Err(EmailServError::EmailInvalid(email.to_string()));
    }

    Ok(())
}

pub fn generate_verification_link(email: &str, key: &[u8; 32], site_url: &str) -> String {
    let token = hex::encode(hash_email(email, key));
    format!("{}/api/verify?token={}", site_url, token)
}

pub fn generate_unsubscribe_link(email: &str, key: &[u8; 32], site_url: &str) -> String {
    let token = hex::encode(hash_email(email, key));
    format!("{}/api/unsubscribe?token={}", site_url, token)
}

pub async fn subscribe(
    State(context): State<ApiContext>,
    Json(body): Json<SubscriptionRequest>,
) -> Result<Response, EmailServError> {
    validate_email(&body.email)?;

    let email_hash = hash_email(&body.email, &context.blake3_key);
    let verification_url =
        generate_verification_link(&body.email, &context.blake3_key, &context.site_url);
    let subject = context
        .email_service
        .load_subject("welcome/verification_body.html")?;
    let context_json = serde_json::json!({
        "subscriber_email": body.email,
        "verification_url": verification_url,
        "site_url": context.site_url
    });

    // One transaction: either the subscriber and its verification email are
    // both stored, or neither is.
    context
        .db
        .subscribe_and_enqueue_verification(
            &email_hash,
            &body.email,
            &subject,
            "welcome/verification_body.html",
            &context_json,
        )
        .await?;

    Ok(StatusCode::OK.into_response())
}

pub async fn verify_subscription(
    State(context): State<ApiContext>,
    Query(params): Query<TokenParams>,
) -> Result<Response, EmailServError> {
    let email_hash = decode_token(&params.token)?;

    if !context.db.verify_subscription(&email_hash).await? {
        return Err(EmailServError::SubscriptionNotFound(params.token));
    }

    // Send the welcome email exactly once, so repeated clicks on the
    // verification link do not produce duplicate mails.
    if let Some(subscription) = context.db.get_subscription(&email_hash).await?
        && !context
            .db
            .has_outbox_item(subscription.id, "welcome")
            .await?
    {
        let unsubscribe_url =
            generate_unsubscribe_link(&subscription.email, &context.blake3_key, &context.site_url);
        let subject = context.email_service.load_subject("welcome/body.html")?;
        let welcome_context = serde_json::json!({
            "subscriber_email": subscription.email,
            "unsubscribe_url": unsubscribe_url,
            "site_url": context.site_url
        });

        context
            .db
            .insert_outbox_item(
                subscription.id,
                &subscription.email,
                "welcome",
                &subject,
                "welcome/body.html",
                &welcome_context,
            )
            .await?;
    }

    Ok(StatusCode::OK.into_response())
}

pub async fn unsubscribe(
    State(context): State<ApiContext>,
    Query(params): Query<TokenParams>,
) -> Result<Response, EmailServError> {
    let email_hash = decode_token(&params.token)?;

    if context.db.delete_subscription(&email_hash).await? {
        Ok(StatusCode::OK.into_response())
    } else {
        Err(EmailServError::SubscriptionNotFound(params.token))
    }
}

pub fn router() -> Router<ApiContext> {
    Router::new()
        .route("/api/subscribe", post(subscribe))
        .route("/api/verify", get(verify_subscription))
        .route("/api/unsubscribe", get(unsubscribe))
}
