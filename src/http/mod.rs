use crate::config::Config;
use axum::{
    Router,
    http::{StatusCode, Uri},
    routing::get,
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::limit::RequestBodyLimitLayer;

mod error;
mod subscription;

#[derive(Clone)]
pub struct ApiContext {
    pub emails: Arc<Mutex<HashMap<String, SubscriptionEmail>>>,
    pub blake3_key: [u8; 32],
}

#[derive(Clone)]
pub struct SubscriptionEmail {
    pub email: String,
    pub is_verified: bool,
}

pub async fn fallback(uri: Uri) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("No route for {uri}"))
}

pub fn create_router(context: ApiContext) -> Router {
    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(10)
            .burst_size(5)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .unwrap(),
    );

    let governor_limiter = governor_conf.limiter().clone();
    let interval = std::time::Duration::from_secs(60);
    // a separate background task to clean up
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);
            tracing::info!("rate limiting storage size: {}", governor_limiter.len());
            governor_limiter.retain_recent();
        }
    });

    let router = Router::new()
        .route("/health_check", get(|| async { StatusCode::OK }))
        .merge(subscription::router())
        .fallback(fallback)
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(GovernorLayer::new(governor_conf))
        .with_state(context);

    router
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let mut key_array = [0u8; 32];
    hex::decode_to_slice(config.blake3_key, &mut key_array as &mut [u8])?;

    let context = ApiContext {
        emails: Arc::new(Mutex::new(HashMap::new())),
        blake3_key: key_array,
    };

    let router = create_router(context);
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let listener = tokio::net::TcpListener::bind(socket).await.unwrap();

    axum::serve(listener, router).await.unwrap();

    Ok(())
}
