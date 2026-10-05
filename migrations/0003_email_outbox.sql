-- Transactional Outbox for emails

CREATE TABLE IF NOT EXISTS email_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email_type TEXT NOT NULL, -- 'verification', 'welcome', 'broadcast'
    recipient_email TEXT NOT NULL,
    subscriber_id INTEGER NOT NULL,
    subject TEXT NOT NULL,
    template_path TEXT NOT NULL,
    context_json TEXT NOT NULL, -- JSON serialized context for Tera
    status TEXT NOT NULL DEFAULT 'pending', -- pending, sent, failed
    retry_count INTEGER DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    processed_at TEXT,
    FOREIGN KEY (subscriber_id) REFERENCES subscriptions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_email_outbox_status ON email_outbox(status);
CREATE INDEX IF NOT EXISTS idx_email_outbox_subscriber_id ON email_outbox(subscriber_id);
