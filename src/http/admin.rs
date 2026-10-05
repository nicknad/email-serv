use axum::Router;
use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use crate::http::ApiContext;
use crate::http::error::EmailServError;
use crate::http::subscription::generate_unsubscribe_link;

const ADMIN_KEY_HEADER: &str = "x-admin-key";

#[derive(serde::Deserialize)]
pub struct BroadcastRequest {
    /// Body template relative to `templates/`, e.g. `newsletters/1/body.html`.
    pub template_path: String,
    /// Optional subject. Defaults to the `subject.txt` next to the template.
    pub subject: Option<String>,
}

/// Compares two byte slices in constant time so the admin key cannot be
/// recovered by timing responses.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn authorize(context: &ApiContext, headers: &HeaderMap) -> Result<(), EmailServError> {
    let provided = headers
        .get(ADMIN_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if !provided.is_empty()
        && constant_time_eq(provided.as_bytes(), context.admin_api_key.as_bytes())
    {
        Ok(())
    } else {
        Err(EmailServError::Unauthorized)
    }
}

pub async fn broadcast(
    State(context): State<ApiContext>,
    headers: HeaderMap,
    Json(body): Json<BroadcastRequest>,
) -> Result<Response, EmailServError> {
    authorize(&context, &headers)?;

    if !body.template_path.starts_with("newsletters/") {
        return Err(EmailServError::BadRequest(
            "template_path must look like 'newsletters/<edition>/body.html'".to_string(),
        ));
    }

    if !context.email_service.template_exists(&body.template_path) {
        return Err(EmailServError::BadRequest(format!(
            "unknown template '{}'",
            body.template_path
        )));
    }

    let subject = match body.subject {
        Some(subject) if !subject.trim().is_empty() => subject,
        _ => context.email_service.load_subject(&body.template_path)?,
    };

    // Render once with sample data before writing any outbox rows, so a broken
    // template fails the request instead of the worker.
    let probe = serde_json::json!({
        "subscriber_email": "probe@example.com",
        "unsubscribe_url": context.site_url,
        "site_url": context.site_url,
    });
    context
        .email_service
        .render_template(&body.template_path, &probe)?;

    let subscribers = context.db.get_all_verified_subscribers().await?;
    let broadcast_id = context
        .db
        .create_broadcast(&subject, &body.template_path)
        .await?;

    for subscriber in &subscribers {
        let unsubscribe_url =
            generate_unsubscribe_link(&subscriber.email, &context.blake3_key, &context.site_url);
        let item_context = serde_json::json!({
            "subscriber_email": subscriber.email,
            "unsubscribe_url": unsubscribe_url,
            "site_url": context.site_url,
        });

        context
            .db
            .enqueue_broadcast_email(
                broadcast_id,
                subscriber.id,
                &subscriber.email,
                &subject,
                &body.template_path,
                &item_context,
            )
            .await?;
    }

    context.db.refresh_broadcast_stats(broadcast_id).await?;

    let queued = subscribers.len();
    tracing::info!(broadcast_id, queued, "Broadcast queued");

    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "broadcast_id": broadcast_id,
            "queued": queued,
        })),
    )
        .into_response())
}

pub fn router() -> Router<ApiContext> {
    Router::new().route("/api/admin/broadcast", post(broadcast))
}
