use axum::Router;
use axum::extract::{State};
use axum::routing::post;

use crate::http::ApiContext;

#[derive(serde::Deserialize)]
struct SubscriptionRequest {
    email: String
}

pub async fn subscribe(State(context): State<ApiContext>) -> String{
    let mut emails = context.emails.lock();
    emails.push("new@example.com".to_string());
    format!("Subscribed! Total: {}", emails.len())
}

pub async fn unsubscribe(State(context): State<ApiContext>) -> String { 
    let mut emails = context.emails.lock();
    if let Some(email) = emails.pop() {
        format!("Unsubscribed: {}", email)
    } else {
        "No emails to unsubscribe".to_string()
    }
}

pub fn router() -> Router<ApiContext> {
    Router::new()
        .route("/api/subscribe", post(subscribe))
        .route("/api/unsubscribe", post(unsubscribe))
}

