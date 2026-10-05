# Email Service Development Guide

This guide provides essential information for agents working on the email-serv Rust project.

## Build, Lint & Test Commands

```bash
# Build and run
cargo run                          # Start the server (port from PORT, .env sample uses 5000)
cargo build                        # Build the project

# Testing
cargo test                         # Run all tests
cargo test --test integration_tests  # Run integration tests only
cargo test test_name                # Run specific test

# Development
cargo check                         # Quick compile check
cargo clippy --all-targets -- -D warnings   # Lint (warnings are errors)
cargo fmt                           # Format code

# Security / supply chain
cargo deny check                    # advisories, licenses, bans, sources (deny.toml)
semgrep scan --config .semgrep.yml --error  # project-specific security rules

# SQLx offline cache (required after changing any sqlx::query! or migration)
cargo sqlx migrate run --database-url sqlite://example.db
cargo sqlx prepare --database-url sqlite://example.db -- --all-targets
```

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.99.0, with rustfmt and
clippy). CI (`.github/workflows/ci.yml`) runs fmt, clippy, tests, cargo-deny and
Semgrep.

## Code Style Guidelines

### Imports and Dependencies
- Use `use` statements at the top of files
- Group imports by standard library, external crates, then local modules
- Prefer full paths over `use` aliases for clarity (e.g., `std::collections::HashMap`)
- Use `anyhow::Result<()>` for main function error propagation

### Types and Naming
- **Functions & Variables**: `snake_case` (e.g., `init_logging`, `create_router`)
- **Types & Structs**: `PascalCase` (e.g., `ApiContext`, `OutboxItem`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `MAX_SEND_ATTEMPTS`, `TEST_BLAKE3_KEY`)
- Use descriptive names that explain purpose
- Prefix async functions with `async fn`

### Error Handling
- Primary error type: `anyhow::Result<T>` for application errors
- Use `?` operator extensively for error propagation
- API errors return JSON format with `EmailServError` enum using `thiserror`
- HTTP handlers return `Result<Response, EmailServError>`, mapped to status codes in `status_code()`
- Database failures map to `EmailServError::DatabaseError`; template failures to `TemplateError`; SMTP failures to `EmailDeliveryFailed`

### Async Patterns
- Mark async functions with `async fn`
- Use `#[tokio::main]` for the main entry point
- `Database` wraps a `sqlx::SqlitePool` and is cheap to clone; `EmailService` wraps `Arc<Tera>` and `Arc<Sender>`
- `ApiContext` is cloned per request via axum `State`
- The outbox worker runs in a `tokio::spawn`ed loop; it must log-and-continue on errors

### Security Considerations
- Email addresses are hashed using keyed Blake3 (32-byte key) for lookups and tokens
- Raw addresses are stored only because mail delivery needs them; never log them (Semgrep enforces this)
- Rate limiting: 10 requests/second, burst of 5 per client IP
- Request body limited to 2 MiB
- Admin endpoint compares `x-admin-key` in constant time
- Credentials loaded from environment variables, never hardcoded (Semgrep enforces this)

### Code Organization
- **Library entry point**: `src/lib.rs` - logging init, outbox worker, `run()`
- **Binary entry point**: `src/main.rs` - calls `email_serv::run()`
- **Configuration**: `src/config.rs` - CLI args and environment parsing
- **Database layer**: `src/database.rs` - SQLx queries, migrations, outbox/broadcast bookkeeping
- **Email layer**: `src/email/mod.rs` (Tera rendering, subject loading) and `src/email/sender.rs` (lettre SMTP)
- **HTTP layer**: `src/http/mod.rs` - router, state, middleware
- **Endpoints**: `src/http/subscription.rs` (subscribe/verify/unsubscribe), `src/http/admin.rs` (broadcast)
- **Error handling**: `src/http/error.rs` - error enum and HTTP mapping
- **Migrations**: `migrations/*.sql` - embedded via `sqlx::migrate!`
- **Templates**: `templates/` - Tera layouts/partials, `body.html` + `subject.txt`
- **Tooling config**: `deny.toml`, `.semgrep.yml`, `.sqlx/` (committed offline query cache)

### Documentation & Comments
- Add comments for complex logic (outbox transaction, retry policy, middleware ordering)
- Document security-related decisions
- Comment external dependencies usage when non-obvious
- Design rationale lives in `docs/DESIGN_DECISIONS.md`; update it when changing architecture

## Testing Guidelines

### Test Structure
- All integration tests in `tests/integration_tests.rs`
- Use `#[tokio::test]` for async tests
- Test helper: `wait_for_server_ready()` for the real-socket rate limit test
- `create_test_context()` builds an in-memory DB with migrations; `create_test_config()` supplies dummy SMTP/admin values

### Test Coverage Requirements
- **All endpoints**: subscribe, verify, unsubscribe, health check, admin broadcast
- **Error cases**: invalid requests, rate limiting, size limits, missing/unknown admin key and template
- **Edge cases**: fallback routes, malformed data, duplicate subscribe, repeated verify
- **Integration**: real server with `TcpListener` for the rate limit burst test

### Test Data
- Use test constants (e.g., `TEST_BLAKE3_KEY`)
- Use in-memory SQLite (`:memory:`) plus `run_migrations()` for isolation
- Test rate limiting with burst patterns (7 requests tests the 5-burst limit)

## Architecture Patterns

### State Management
- SQLite via `sqlx::SqlitePool`; clone the pool, no global mutex
- Email identifiers are 32-byte keyed Blake3 hashes (`email_hash BLOB UNIQUE`)
- Email is queued through `email_outbox`; the worker drains it and updates `broadcast_recipients`/`broadcasts`

### HTTP Handler Pattern
```rust
pub async fn handler(
    State(context): State<ApiContext>,
    Json(payload): Json<RequestType>,
) -> Result<Response, EmailServError> {
    // Handler logic
    // Return Result<Response, EmailServError>
}
```

### Configuration
- CLI args parsed with `clap` using `derive` macros
- Environment variables via `.env` (loaded with `dotenvy`)
- Required: `db_conn`, `port`, `blake3_key`, `smtp_host`, `smtp_port`, `smtp_user`, `smtp_pass`, `email_from`, `admin_api_key`
- Optional: `log_dir`, `site_url` (default `http://localhost:3000`)

### Middleware Stack (order matters)
Layers are applied in the order written, and execute in reverse (last added runs
first). In `create_router`:
1. routes + state (`.with_state()`)
2. `RequestBodyLimitLayer` (2 MiB)
3. `GovernorLayer` (rate limit, outermost)
Fallback route is registered before the layers.

## Development Notes

### Current Limitations & TODOs
- SMTP delivery is not covered by tests (needs a mock server or an injected `Sender` trait)
- Verification/unsubscribe tokens never expire and cannot be rotated
- Broadcast handler loads all verified subscribers into memory; no pagination/progress endpoint
- Outbox worker polls every 10s; no push-based wakeup
- Email validation is basic (`@` presence + length checks)
- SQLite single-writer; connection-pool tuning not implemented

### Security Hardening Checklist
- [x] Proper email validation
- [x] Comprehensive, typed error responses
- [x] Keyed hashing + stateless verify/unsubscribe tokens
- [x] Rate limiting with storage cleanup
- [x] PII-safe structured logging (Semgrep-enforced)
- [x] cargo-deny advisories/licences/bans/sources in CI
- [ ] Token expiry/rotation
- [ ] Mock-SMTP integration tests
- [ ] Metrics/alerts for permanently failed outbox items

### Environment Setup
- Copy `.env.sample` to `.env` and fill in all required variables (SMTP + admin key included)
- Default server binds `127.0.0.1` on `PORT`
- Logs write JSON to `LOG_DIR` (daily rolling)

## Docker Deployment

### Quick Start
```bash
cp .env.sample .env   # fill in required values; compose reads it automatically
docker compose up -d
```

### Build and Deploy
```bash
# Use deploy script
./deploy.sh --registry registry.digitalocean.com/user --tag v1.0.0

# Or build manually
docker build -t registry.digitalocean.com/user/email-serv:latest .
docker push registry.digitalocean.com/user/email-serv:latest
```

### Container Configuration
- **Build image**: rust:1.99-alpine; copies `.sqlx/` + `migrations/` and builds with `SQLX_OFFLINE=true`
- **Runtime image**: Alpine Linux 3.20 (minimal), templates copied to `/app/templates`
- **User**: Non-root `app:app` (uid 1000)
- **Exposed Port**: 8080
- **Health Check**: `/health_check` endpoint
- **Volumes**:
  - `/app/data` - SQLite database (persist)
  - `/app/logs` - Application logs (persist)

### Cloud Deployment
See `DOCKER_DEPLOYMENT.md` for guides covering AWS ECS/Fargate, Google Cloud
Run, Azure Container Instances and DigitalOcean App Platform.

### Security Features
- Non-root user execution
- Minimal attack surface (Alpine base)
- Health monitoring built-in
