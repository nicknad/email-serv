use sqlx::sqlite::SqlitePool;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

pub struct OutboxItem {
    pub id: i64,
    pub email_type: String,
    pub recipient_email: String,
    pub subscriber_id: i64,
    pub subject: String,
    pub template_path: String,
    pub context_json: String,
    pub retry_count: i32,
    pub broadcast_id: Option<i64>,
}

pub struct Subscriber {
    pub id: i64,
    pub email: String,
}

/// Number of send attempts before an outbox item is marked permanently failed.
pub const MAX_SEND_ATTEMPTS: i32 = 3;

impl Database {
    pub async fn new(db_url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePool::connect(db_url).await?;
        Ok(Database { pool })
    }

    pub async fn insert_subscription(&self, email_hash: &[u8], email: &str) -> anyhow::Result<i64> {
        let res = sqlx::query!(
            "INSERT OR IGNORE INTO subscriptions (email_hash, email, is_verified) VALUES (?, ?, 0)",
            email_hash,
            email,
        )
        .execute(&self.pool)
        .await?;

        if res.rows_affected() == 0 {
            let row = sqlx::query!(
                "SELECT id as \"id!: i64\" FROM subscriptions WHERE email_hash = ?",
                email_hash
            )
            .fetch_one(&self.pool)
            .await?;
            Ok(row.id)
        } else {
            Ok(res.last_insert_rowid())
        }
    }

    /// Creates the subscription (or reuses it) and queues a verification email
    /// in a single transaction, so a saved subscription can never exist
    /// without its verification email being queued.
    pub async fn subscribe_and_enqueue_verification(
        &self,
        email_hash: &[u8],
        email: &str,
        subject: &str,
        template_path: &str,
        context: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let context_str = serde_json::to_string(context)?;
        let mut tx = self.pool.begin().await?;

        sqlx::query!(
            "INSERT OR IGNORE INTO subscriptions (email_hash, email, is_verified) VALUES (?, ?, 0)",
            email_hash,
            email,
        )
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query!(
            "SELECT id as \"id!: i64\", is_verified FROM subscriptions WHERE email_hash = ?",
            email_hash
        )
        .fetch_one(&mut *tx)
        .await?;

        let subscriber_id = row.id;

        if row.is_verified == 0 {
            let already_queued = sqlx::query!(
                "SELECT 1 as \"one: i64\" FROM email_outbox WHERE subscriber_id = ? AND email_type = 'verification' LIMIT 1",
                subscriber_id
            )
            .fetch_optional(&mut *tx)
            .await?;

            if already_queued.is_none() {
                sqlx::query!(
                    "INSERT INTO email_outbox (subscriber_id, recipient_email, email_type, subject, template_path, context_json) VALUES (?, ?, 'verification', ?, ?, ?)",
                    subscriber_id,
                    email,
                    subject,
                    template_path,
                    context_str
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    /// Whether any outbox entry (pending, sent or failed) of `email_type`
    /// exists for this subscriber. Used to keep one-off emails idempotent.
    pub async fn has_outbox_item(
        &self,
        subscriber_id: i64,
        email_type: &str,
    ) -> anyhow::Result<bool> {
        let res = sqlx::query!(
            "SELECT 1 as \"one: i64\" FROM email_outbox WHERE subscriber_id = ? AND email_type = ? LIMIT 1",
            subscriber_id,
            email_type
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(res.is_some())
    }

    pub async fn insert_outbox_item(
        &self,
        subscriber_id: i64,
        recipient_email: &str,
        email_type: &str,
        subject: &str,
        template_path: &str,
        context: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let context_str = serde_json::to_string(context)?;
        sqlx::query!(
            "INSERT INTO email_outbox (subscriber_id, recipient_email, email_type, subject, template_path, context_json) VALUES (?, ?, ?, ?, ?, ?)",
            subscriber_id,
            recipient_email,
            email_type,
            subject,
            template_path,
            context_str
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_pending_outbox_items(&self) -> anyhow::Result<Vec<OutboxItem>> {
        let rows = sqlx::query!(
            "SELECT id as \"id!: i64\", email_type, recipient_email, subscriber_id as \"subscriber_id!: i64\", subject, template_path, context_json, retry_count as \"retry_count!: i32\", broadcast_id FROM email_outbox WHERE status = 'pending' ORDER BY created_at ASC LIMIT 50"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OutboxItem {
                id: r.id,
                email_type: r.email_type,
                recipient_email: r.recipient_email,
                subscriber_id: r.subscriber_id,
                subject: r.subject,
                template_path: r.template_path,
                context_json: r.context_json,
                retry_count: r.retry_count,
                broadcast_id: r.broadcast_id,
            })
            .collect())
    }

    /// Records a failed send attempt. Returns `true` when the item was put
    /// back into the queue for another attempt, `false` when it is now
    /// permanently failed.
    pub async fn requeue_or_fail_outbox(
        &self,
        id: i64,
        error: &str,
        max_attempts: i32,
    ) -> anyhow::Result<bool> {
        let row = sqlx::query!(
            "UPDATE email_outbox
             SET retry_count = retry_count + 1,
                 last_error = ?,
                 status = CASE WHEN retry_count + 1 < ? THEN 'pending' ELSE 'failed' END
             WHERE id = ?
             RETURNING status as \"status!: String\"",
            error,
            max_attempts,
            id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(row.status == "pending")
    }

    pub async fn mark_outbox_sent(&self, id: i64) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query!(
            "UPDATE email_outbox SET status = 'sent', processed_at = ? WHERE id = ?",
            now,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_outbox_failed(&self, id: i64, error: &str) -> anyhow::Result<()> {
        sqlx::query!(
            "UPDATE email_outbox SET status = 'failed', last_error = ?, retry_count = retry_count + 1 WHERE id = ?",
            error,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_subscription(&self, email_hash: &[u8]) -> anyhow::Result<Option<Subscriber>> {
        let row = sqlx::query!(
            "SELECT id as \"id!: i64\", email FROM subscriptions WHERE email_hash = ?",
            email_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Subscriber {
            id: r.id,
            email: r.email,
        }))
    }

    pub async fn verify_subscription(&self, email_hash: &[u8]) -> anyhow::Result<bool> {
        let result = sqlx::query!(
            "UPDATE subscriptions 
            SET is_verified = 1, updated_at = datetime('now') 
            WHERE email_hash = ?",
            email_hash
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_subscription(&self, email_hash: &[u8]) -> anyhow::Result<bool> {
        let result = sqlx::query!("DELETE FROM subscriptions WHERE email_hash = ?", email_hash)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_all_verified_subscribers(&self) -> anyhow::Result<Vec<Subscriber>> {
        let rows = sqlx::query!(
            "SELECT id as \"id!: i64\", email FROM subscriptions WHERE is_verified = 1"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Subscriber {
                id: r.id,
                email: r.email,
            })
            .collect())
    }

    pub async fn create_broadcast(
        &self,
        subject: &str,
        template_path: &str,
    ) -> anyhow::Result<i64> {
        let res = sqlx::query!(
            "INSERT INTO broadcasts (subject, template_path, status) VALUES (?, ?, 'pending')",
            subject,
            template_path
        )
        .execute(&self.pool)
        .await?;

        Ok(res.last_insert_rowid())
    }

    /// Queues one broadcast email for a subscriber and records the delivery in
    /// `broadcast_recipients` atomically.
    pub async fn enqueue_broadcast_email(
        &self,
        broadcast_id: i64,
        subscriber_id: i64,
        recipient_email: &str,
        subject: &str,
        template_path: &str,
        context: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let context_str = serde_json::to_string(context)?;
        let mut tx = self.pool.begin().await?;

        sqlx::query!(
            "INSERT INTO email_outbox (subscriber_id, recipient_email, email_type, subject, template_path, context_json, broadcast_id) VALUES (?, ?, 'broadcast', ?, ?, ?, ?)",
            subscriber_id,
            recipient_email,
            subject,
            template_path,
            context_str,
            broadcast_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "INSERT INTO broadcast_recipients (broadcast_id, subscriber_id, status) VALUES (?, ?, 'pending')",
            broadcast_id,
            subscriber_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_broadcast_recipient(
        &self,
        broadcast_id: i64,
        subscriber_id: i64,
        status: &str,
        error_message: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query!(
            "UPDATE broadcast_recipients SET status = ?, error_message = ? WHERE broadcast_id = ? AND subscriber_id = ?",
            status,
            error_message,
            broadcast_id,
            subscriber_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Recomputes the counters shown for a broadcast from the per-recipient
    /// rows. The broadcast is completed once no recipient is pending.
    pub async fn refresh_broadcast_stats(&self, broadcast_id: i64) -> anyhow::Result<()> {
        let counts = sqlx::query!(
            "SELECT
                COUNT(*) as \"total!: i64\",
                COALESCE(SUM(CASE WHEN status = 'sent' THEN 1 ELSE 0 END), 0) as \"sent!: i64\",
                COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) as \"failed!: i64\",
                COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) as \"pending!: i64\"
             FROM broadcast_recipients WHERE broadcast_id = ?",
            broadcast_id
        )
        .fetch_one(&self.pool)
        .await?;

        let status = if counts.pending == 0 {
            "completed"
        } else {
            "sending"
        };

        self.update_broadcast_stats(
            broadcast_id,
            counts.total as i32,
            counts.sent as i32,
            counts.failed as i32,
            status,
        )
        .await
    }

    pub async fn update_broadcast_stats(
        &self,
        broadcast_id: i64,
        total_recipients: i32,
        success_count: i32,
        failure_count: i32,
        status: &str,
    ) -> anyhow::Result<()> {
        let completed_at = if status == "completed" || status == "failed" {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };

        sqlx::query!(
            "UPDATE broadcasts SET total_recipients = ?, success_count = ?, failure_count = ?, status = ?, completed_at = ? WHERE id = ?",
            total_recipients,
            success_count,
            failure_count,
            status,
            completed_at,
            broadcast_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}
