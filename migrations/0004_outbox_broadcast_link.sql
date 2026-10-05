-- Link outbox emails to the broadcast they belong to.
-- Verification/welcome emails keep broadcast_id = NULL.

ALTER TABLE email_outbox ADD COLUMN broadcast_id INTEGER REFERENCES broadcasts(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_email_outbox_broadcast_id ON email_outbox(broadcast_id);
