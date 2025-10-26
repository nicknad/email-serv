use crate::config::Config;
use axum::{
    Router,
    http::{StatusCode, Uri},
    routing::get,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
mod error;

#[derive(Clone)]
pub struct ApiContext {
    emails: Arc<Vec<String>>,
}

pub async fn fallback(uri: Uri) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("No route for {uri}"))
}

pub fn create_router(context: ApiContext) -> Router {
    Router::new()
        .route("/health_check", get(|| async { StatusCode::OK }))
        .fallback(fallback)
        .with_state(context)
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let app = create_router(ApiContext {
        emails: Arc::new(Vec::new()),
    });
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let listener = tokio::net::TcpListener::bind(socket).await.unwrap();

    axum::serve(listener, app).await.unwrap();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::Request, http::StatusCode};
    use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`

    #[tokio::test]
    async fn test_fallback() {
        let app = create_router(ApiContext {
            emails: Arc::new(Vec::new()),
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/gibberish")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = create_router(ApiContext {
            emails: Arc::new(Vec::new()),
        });
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health_check")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
