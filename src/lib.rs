use crate::config::Config;
use crate::database::{Database, MAX_SEND_ATTEMPTS, OutboxItem};
use crate::email::EmailService;
use crate::http::error::EmailServError;
use clap::Parser;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{Layer, prelude::*};

pub mod config;
pub mod database;
pub mod email;
pub mod http;

pub fn init_logging(config: &Config) -> Result<WorkerGuard, EmailServError> {
    let log_dir = std::path::Path::new(&config.log_dir);
    std::fs::create_dir_all(log_dir)?;

    let file_appender = tracing_appender::rolling::RollingFileAppender::new(
        tracing_appender::rolling::Rotation::DAILY,
        log_dir,
        "api.log",
    );

    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

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

    tracing::info!(
        "Tracing initialized and writing to file: {}",
        log_dir.join("api.log").display()
    );

    Ok(guard)
}

async fn update_broadcast_progress(
    db: &Database,
    item: &OutboxItem,
    status: &str,
    error: Option<&str>,
) {
    let Some(broadcast_id) = item.broadcast_id else {
        return;
    };

    if let Err(e) = db
        .mark_broadcast_recipient(broadcast_id, item.subscriber_id, status, error)
        .await
    {
        tracing::error!("Failed to update broadcast recipient: {}", e);
        return;
    }

    if let Err(e) = db.refresh_broadcast_stats(broadcast_id).await {
        tracing::error!("Failed to refresh broadcast stats: {}", e);
    }
}

/// Marks an outbox item as failed without retrying. Used for errors that
/// cannot succeed on a retry (invalid JSON, missing template).
async fn record_permanent_failure(db: &Database, item: &OutboxItem, error: &str) {
    if let Err(e) = db.mark_outbox_failed(item.id, error).await {
        tracing::error!("Failed to mark outbox item {} as failed: {}", item.id, e);
    }
    update_broadcast_progress(db, item, "failed", Some(error)).await;
}

pub async fn start_outbox_worker(db: Database, email_service: EmailService) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    loop {
        interval.tick().await;

        let items = match db.get_pending_outbox_items().await {
            Ok(items) => items,
            Err(e) => {
                tracing::error!("Failed to fetch outbox items: {}", e);
                continue;
            }
        };

        for item in items {
            let context: serde_json::Value = match serde_json::from_str(&item.context_json) {
                Ok(context) => context,
                Err(e) => {
                    record_permanent_failure(&db, &item, &format!("JSON error: {}", e)).await;
                    continue;
                }
            };

            let rendered = match email_service.render_template(&item.template_path, &context) {
                Ok(rendered) => rendered,
                Err(e) => {
                    record_permanent_failure(&db, &item, &format!("Render error: {}", e)).await;
                    continue;
                }
            };

            match email_service
                .send_email(&item.recipient_email, &item.subject, &rendered)
                .await
            {
                Ok(_) => {
                    if let Err(e) = db.mark_outbox_sent(item.id).await {
                        tracing::error!("Failed to mark outbox item {} as sent: {}", item.id, e);
                    }
                    update_broadcast_progress(&db, &item, "sent", None).await;
                }
                Err(e) => {
                    let error = format!("Send error: {}", e);
                    match db
                        .requeue_or_fail_outbox(item.id, &error, MAX_SEND_ATTEMPTS)
                        .await
                    {
                        Ok(true) => {
                            tracing::warn!(
                                outbox_id = item.id,
                                "Send attempt {} failed, retrying: {}",
                                item.retry_count + 1,
                                error
                            );
                        }
                        Ok(false) => {
                            tracing::error!(
                                outbox_id = item.id,
                                "Send failed permanently after {} attempts: {}",
                                item.retry_count + 1,
                                error
                            );
                            update_broadcast_progress(&db, &item, "failed", Some(&error)).await;
                        }
                        Err(e) => {
                            tracing::error!("Failed to record outbox failure: {}", e);
                        }
                    }
                }
            }
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::parse();
    let _guard = init_logging(&config)?;

    let db = Database::new(&config.db_conn)
        .await
        .map_err(EmailServError::DatabaseError)?;

    db.run_migrations().await?;

    let email_service = EmailService::new(&config)?;

    tokio::spawn(start_outbox_worker(db.clone(), email_service.clone()));

    http::serve(config, db, email_service).await?;

    Ok(())
}
