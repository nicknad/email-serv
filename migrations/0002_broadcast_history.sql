-- Broadcast history migration

-- Represents a single email campaign sent to subscribers
CREATE TABLE IF NOT EXISTS broadcasts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject TEXT NOT NULL,
    template_path TEXT NOT NULL, -- Path to the specific newsletter version (e.g., 'newsletters/1/body.html')
    status TEXT NOT NULL DEFAULT 'pending', -- pending, sending, completed, failed
    total_recipients INTEGER DEFAULT 0,
    success_count INTEGER DEFAULT 0,
    failure_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT
);

-- Tracks individual delivery status for each subscriber per broadcast
CREATE TABLE IF NOT EXISTS broadcast_recipients (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    broadcast_id INTEGER NOT NULL,
    subscriber_id INTEGER NOT NULL,
    status TEXT NOT NULL, -- sent, failed, rejected
    error_message TEXT,
    sent_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (broadcast_id) REFERENCES broadcasts(id) ON DELETE CASCADE,
    FOREIGN KEY (subscriber_id) REFERENCES subscriptions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_broadcast_recipients_broadcast_id ON broadcast_recipients(broadcast_id);
