use crate::config::Config;
use axum::{Router, http::StatusCode, routing::get};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
mod error;

pub fn init_router() -> Router {
    Router::new().route("/health_check", get(|| async { StatusCode::OK }))
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let app = init_router();
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let listener = tokio::net::TcpListener::bind(socket).await.unwrap();

    axum::serve(listener, app).await.unwrap();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::Request, http::StatusCode};

    #[tokio::test]
    async fn test_health_check() {
        let app = init_router();

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
