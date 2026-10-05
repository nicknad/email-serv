# Email Send Component Development Plan

This plan outlines the implementation of a broadcast module for `email-serv` that renders HTML emails from templates and sends them to all verified subscribers.

## 1. Core Objectives
- Implement a templating engine to generate rich HTML emails.
- Integrate an SMTP client for reliable email delivery.
- Create an administrative API to trigger broadcast campaigns.
- Ensure efficient batch processing for sending emails to many subscribers.
- **Reliable Verification:** Ensure every new subscriber receives exactly one verification email, with no duplicates for verified users.

## 2. Technical Stack
- **Templating Engine:** [Tera](https://github.com/Keats/tera) (Jinja2-like templates for Rust).
- **Email Library:** [Lettre](https://github.com/lettre/lettre) (The standard SMTP implementation for Rust).
- **Database:** SQLite (via SQLx) for subscriber retrieval and outbox management.

## 3. Implementation Steps

### Phase 1: Database Enhancements (`src/database.rs`)
- Add `get_all_verified_subscribers()` to fetch a list of all emails with `is_verified = 1`.
- Implement **Broadcast Tracking** tables (`broadcasts`, `broadcast_recipients`).
- Implement **Transactional Outbox** table:
    - `email_outbox`: To track emails that need to be sent (verification, welcome, etc.) independently of the HTTP request cycle.

#### Outbox Schema Details:
```sql
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
```

### Phase 2: Configuration Updates (`src/config.rs`)
Add required SMTP settings to the `Config` struct (Host, Port, User, Pass, From, Admin Key).

### Phase 3: Templating & Rendering (`src/email/mod.rs`)
- Initialize a `Tera` instance to load templates from a `templates/` directory.
- Implement a `render_template` function that injects dynamic data.
- **Organization:** Use numbered subfolders for newsletters (e.g., `newsletters/1/`).

### Phase 4: Email Sending Logic (`src/email/sender.rs`)
- Implement a `Sender` struct that manages the `lettre::SmtpTransport`.
- **Implement Outbox Worker:**
    - A background loop that polls `email_outbox` for `pending` items.
    - Renders and sends emails, updating status to `sent` or `failed`.
- Create an asynchronous `broadcast` function that populates the outbox in bulk.

### Phase 5: Admin API & Routing (`src/http/admin.rs`)
- Create a new router for admin operations.
- Implement `POST /api/admin/broadcast`:
    - Requires `ADMIN_API_KEY` authentication.
    - Triggers the broadcast logic by inserting records into the outbox.

### Phase 6: Automated Verification Flow
- **Idempotent Subscription:**
    - When `POST /api/subscribe` is called:
        1. Check if the email exists.
        2. If `is_verified = 1`, return OK immediately (do nothing).
        3. If `is_verified = 0`, check `email_outbox` for an existing `pending` verification email for this user.
        4. If no pending email exists, insert a new `verification` entry into `email_outbox` and return OK.
- **Reliability Recommendations:**
    - **Transactional Outbox (Selected):** Guarantees that if the subscription is saved, the email is queued. Prevents "ghost" subscriptions or double-sending if the API crashes.
    - **Cooldown Period:** Add a `last_sent_at` check to allow users to "resend" verification if they didn't get it, but only after e.g., 5 minutes.

## 4. Testing Strategy
- **Unit Tests:** Verify template rendering with various context data.
- **Mocking:** Use a mock SMTP server (like Mailtrap or Mailhog) for integration testing.
- **Outbox Tests:** Verify that `POST /api/subscribe` correctly populates the outbox and avoids duplicates.

## 5. Security Considerations
- **Rate Limiting:** Prevent abuse of the subscription endpoint.
- **Unsubscribe Links:** Every broadcast email MUST include a unique, secure unsubscribe link.
- **Admin Security:** Protect the broadcast endpoint with a strong API key.
