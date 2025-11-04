use clap::Parser;
use email_serv::config::Config;
use email_serv::http;
use tracing_subscriber::{Layer, prelude::*};

fn init_logging(config: &Config) {
    let log_dir = std::path::Path::new(&config.log_dir);

    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir)
            .expect(format!("Failed to create log directory {}", config.log_dir).as_str());
    }

    let file_appender = tracing_appender::rolling::RollingFileAppender::new(
        tracing_appender::rolling::Rotation::DAILY, 
        log_dir,
        "api.log", 
    );
    
    let (non_blocking_appender, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::Registry::default()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tower_http=debug")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking_appender)
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(tracing_subscriber::EnvFilter::new("error")),
        )
        .init();
        
    tracing::info!("Tracing initialized and writing to file: {}", log_dir.join("api.log").display());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let config = Config::parse();
    init_logging(&config);

    http::serve(config).await?;

    Ok(())
}
