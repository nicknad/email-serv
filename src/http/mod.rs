use crate::config::Config;
use axum::{
    Router,
    http::{StatusCode, Uri},
    routing::get,
};
use std::{collections::HashMap, net::{IpAddr, Ipv4Addr, SocketAddr}};
use std::sync::Arc;
use parking_lot::Mutex;
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
   let router = Router::new()
        .route("/health_check", get(|| async { StatusCode::OK }))
        .merge(subscription::router())
        .fallback(fallback)
        .with_state(context);

    router
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let mut key_array = [0u8; 32];
    hex::decode_to_slice(config.blake3_key, &mut key_array as &mut[u8])?;

    let context = ApiContext { 
            emails: Arc::new(Mutex::new(HashMap::new())), 
            blake3_key: key_array
    };

    let router = create_router(context);
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let listener = tokio::net::TcpListener::bind(socket).await.unwrap();

    axum::serve(listener, router).await.unwrap();

    Ok(())
}
