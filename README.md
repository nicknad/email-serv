# Email-Serv

A lightweight Rust backend for managing mailing-list subscriptions and sending
templated broadcast emails. Built as a learning project to explore **Rust**
end-to-end: async web services, compile-time-checked SQL, background workers,
templating, SMTP, testing, supply-chain checks and deployment.

> **Design notes:** the decisions behind the architecture, the trade-offs, and
> what can be learned from them are documented in
> [`docs/DESIGN_DECISIONS.md`](docs/DESIGN_DECISIONS.md).

## Features

- **Subscribe / verify / unsubscribe** with a double opt-in flow.
- **Privacy-first identifiers:** emails are looked up by a keyed [Blake3](https://github.com/BLAKE3-team/BLAKE3) hash; raw addresses never appear in logs.
- **Transactional outbox:** HTTP requests only queue email; a background worker renders and delivers it, with retries and per-item status.
- **Templated emails:** [Tera](https://github.com/Keats/tera) layouts/partials, `body.html` + `subject.txt` per template.
- **Admin broadcasts:** one authenticated endpoint queues a newsletter for every verified subscriber, with a unique unsubscribe link per mail.
- **Broadcast history:** per-campaign and per-recipient delivery tracking.
- **Operational basics:** structured JSON logs, per-IP rate limiting, 2 MiB body limit, health check.
- **CI & supply chain:** `cargo-deny` (advisories/licenses/bans/sources) and custom Semgrep rules for project security conventions.

## How it works

```text
HTTP request ──▶ SQLite transaction ──▶ email_outbox ──▶ worker (10s poll) ──▶ SMTP
                 subscription + email      (pending)      Tera render + lettre
                                                          retry ≤ 3 → sent/failed
```

1. `POST /api/subscribe` creates the subscriber and queues a verification email in one transaction.
2. The worker renders the template and sends it via SMTP, updating outbox and broadcast bookkeeping.
3. `GET /api/verify?token=…` marks the subscriber verified and queues exactly one welcome email.
4. `POST /api/admin/broadcast` queues one personalized email per verified subscriber.

## Quick start

```bash
git clone https://github.com/nicknad/email-serv
cd email-serv

cp .env.sample .env          # then fill in BLAKE3_KEY, SMTP_*, ADMIN_API_KEY, …

cargo run                    # starts on http://127.0.0.1:5000 by default
cargo test                   # 27 integration tests (in-memory SQLite)
```

Any SMTP catcher works for local development, e.g.
[`mailpit`](https://github.com/axllent/mailpit) or
[`mailhog`](https://github.com/mailhog/MailHog): point `SMTP_HOST=localhost`,
`SMTP_PORT=1025` at it.

## API

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/health_check` | – | Returns `200 OK` |
| `POST` | `/api/subscribe` | – | Body `{ "email": "user@example.com" }`; idempotent |
| `GET` | `/api/verify?token=<64 hex>` | – | Double opt-in confirmation |
| `GET` | `/api/unsubscribe?token=<64 hex>` | – | Removes the subscription |
| `POST` | `/api/admin/broadcast` | `x-admin-key` | Queues a newsletter for all verified subscribers |

### Examples

```bash
curl -X POST http://localhost:5000/api/subscribe \
  -H 'Content-Type: application/json' \
  -d '{"email":"user@example.com"}'

curl -X POST http://localhost:5000/api/admin/broadcast \
  -H 'Content-Type: application/json' \
  -H 'x-admin-key: <ADMIN_API_KEY>' \
  -d '{"template_path":"newsletters/1/body.html","subject":"Edition #1"}'
# => 202 Accepted {"broadcast_id":1,"queued":42}
```

If `subject` is omitted, the worker loads `templates/<…>/subject.txt` next to
the body template.

## Configuration

Loaded via CLI flags or environment variables (`clap` `env`), `.env` is read at
startup. See [`.env.sample`](.env.sample).

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DB_CONN` | yes | – | SQLite connection string, e.g. `sqlite://example.db` |
| `BLAKE3_KEY` | yes | – | 64 hex chars (32 bytes) used for keyed email hashing |
| `PORT` | yes | – | HTTP port |
| `SMTP_HOST` / `SMTP_PORT` | yes | – | SMTP relay host and port |
| `SMTP_USER` / `SMTP_PASS` | yes | – | SMTP credentials |
| `EMAIL_FROM` | yes | – | `From:` address |
| `ADMIN_API_KEY` | yes | – | Pre-shared key for the admin endpoint |
| `LOG_DIR` | no | – | Directory for the daily rolling JSON log |
| `SITE_URL` | no | `http://localhost:3000` | Public base URL used in links |

## Development

```bash
cargo fmt                                              # formatting
cargo clippy --all-targets -- -D warnings              # lints
cargo test                                             # tests
cargo deny check                                       # advisories/licences/bans/sources
semgrep scan --config .semgrep.yml --error             # project security rules
```

The toolchain is pinned in [`rust-toolchain.toml`](rust-toolchain.toml)
(currently Rust 1.99.0; see
[`docs/DESIGN_DECISIONS.md`](docs/DESIGN_DECISIONS.md#212-toolchain-pinning-and-dependency-updates)
for why this is not an "LTS" release).

### Changing SQL

Queries use `sqlx::query!`, which normally checks SQL at compile time against a
live database. CI has no database, so the offline cache in `.sqlx/` is used:

```bash
# after editing any sqlx::query! invocation or migration:
cargo sqlx migrate run --database-url sqlite://example.db
cargo sqlx prepare --database-url sqlite://example.db -- --all-targets
# commit the updated .sqlx/ directory
```

## Docker

```bash
cp .env.sample .env   # fill it in
docker compose up -d
curl http://localhost:8080/health_check
```

See [`DOCKER_DEPLOYMENT.md`](DOCKER_DEPLOYMENT.md) for cloud deployment guides.

## Security notes

- Emails are hashed with a keyed Blake3 hash for storage lookups and links; logs must never contain raw addresses (enforced by Semgrep).
- Verification/unsubscribe links are stateless tokens derived from that hash.
- The admin endpoint uses a constant-time comparison against `ADMIN_API_KEY`.
- Rate limiting: 10 req/s with a burst of 5 per client IP; request bodies capped at 2 MiB.
- `cargo-deny` fails the build on known vulnerabilities, unmaintained crates, disallowed licenses, and non-crates.io sources.

## License

MIT License © 2025 Nick
