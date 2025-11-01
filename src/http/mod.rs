use crate::config::Config;
use axum::{
    Router,
    http::{StatusCode, Uri},
    routing::get,
    routing::IntoMakeService,
    extract::State,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use parking_lot::Mutex;
mod error;
mod subscription;

#[derive(Clone)]
pub struct ApiContext {
    pub emails: Arc<Mutex<Vec<String>>>,
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
    let context = ApiContext { emails: Arc::new(Mutex::new(Vec::new())) };
    let router = create_router(context);
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let listener = tokio::net::TcpListener::bind(socket).await.unwrap();

    axum::serve(listener, router).await.unwrap();

    Ok(())
}
