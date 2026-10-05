use crate::config::Config;
use crate::database::Database;
use axum::{
    Router,
    http::{StatusCode, Uri},
    response::IntoResponse,
    routing::get,
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::limit::RequestBodyLimitLayer;

pub mod admin;
pub mod error;
pub mod subscription;

pub use error::EmailServError;

use crate::email::EmailService;

#[derive(Clone)]
pub struct ApiContext {
    pub db: Database,
    pub blake3_key: [u8; 32],
    pub site_url: String,
    pub admin_api_key: String,
    pub email_service: EmailService,
}

pub async fn fallback(uri: Uri) -> impl IntoResponse {
    (StatusCode::NOT_FOUND, format!("No route for {uri}"))
}

/// Builds the application router.
///
/// Middleware order (outermost first):
/// 1. rate limiting (per client IP)
/// 2. request body limit (2 MiB)
/// 3. routes
pub fn create_router(context: ApiContext) -> Router {
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(10)
            .burst_size(5)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("Rate limit config failed"),
    );

    // Periodically drop rate-limit entries for clients that stopped calling,
    // otherwise the in-memory map grows without bound.
    let limiter = governor_conf.limiter().clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            tracing::debug!("Rate limiting storage size: {}", limiter.len());
            limiter.retain_recent();
        }
    });

    Router::new()
        .route("/health_check", get(|| async { StatusCode::OK }))
        .merge(subscription::router())
        .merge(admin::router())
        .fallback(fallback)
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(GovernorLayer::new(governor_conf))
        .with_state(context)
}

pub async fn serve(
    config: Config,
    db: Database,
    email_service: EmailService,
) -> anyhow::Result<()> {
    let mut key_array = [0u8; 32];
    hex::decode_to_slice(&config.blake3_key, &mut key_array)?;

    let context = ApiContext {
        db,
        blake3_key: key_array,
        site_url: config.site_url,
        admin_api_key: config.admin_api_key,
        email_service,
    };

    let router = create_router(context);
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let listener = tokio::net::TcpListener::bind(socket).await?;

    tracing::info!("Server listening on {}", socket);
    axum::serve(listener, router).await?;

    Ok(())
}
