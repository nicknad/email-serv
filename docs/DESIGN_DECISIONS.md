# Design Decisions & Learnings

`email-serv` is a deliberately small mailing-list backend written in Rust. Its
purpose is to explore how far a single, well-structured Rust service can get
with a modest dependency set: HTTP, persistence, templating, SMTP, background
processing, supply-chain checks and tests. This document records **what was
decided, why, which trade-offs were accepted, and what can be learned from the
result**.

---

## 1. Architecture at a glance

```
                    ┌──────────────────────────────────────────┐
  HTTP (axum)       │  POST /api/subscribe   GET /api/verify    │
  rate limit 10/s   │  GET /api/unsubscribe  POST /api/admin/…  │
  body limit 2 MiB  └───────────────┬──────────────────────────┘
                                    │ enqueue (single transaction)
                                    ▼
                    ┌──────────────────────────────────────────┐
  SQLite (sqlx)     │ subscriptions · email_outbox · broadcasts │
                    │          broadcast_recipients             │
                    └───────────────┬──────────────────────────┘
                                    │ poll every 10 s (batch of 50)
                                    ▼
  worker (tokio)    ┌──────────────────────────────────────────┐
                    │ render with Tera → send with lettre       │
                    │ retry ≤ 3 → sent / permanently failed     │
                    └──────────────────────────────────────────┘
```

The request path never talks to the SMTP server directly. It only writes a row
to the **transactional outbox**; a background worker drains the outbox.

---

## 2. Decisions

Each decision is written as context → decision → why → trade-offs.

### 2.1 Axum + Tokio for HTTP

- **Context:** Need an HTTP API with JSON handlers, middleware (rate limiting,
  body limits) and straightforward integration testing.
- **Decision:** `axum` 0.8 on top of `tokio`.
- **Why:** Axum is a thin layer over `hyper`/`tower`, so middleware compose as
  ordinary `tower` layers. Extractors (`State`, `Json`, `Query`, `HeaderMap`)
  keep handlers small, and `Router` implements `tower::Service`, which allows
  `oneshot` tests without binding a socket. It also has no macro-heavy routing.
- **Trade-offs:** Smaller ecosystem than `actix-web`; the tower layer stack is
  easy to mis-order (state must be attached last, rate limiting is layered
  before routing, etc.).

### 2.2 SQLite + SQLx (no ORM)

- **Context:** A single-node example service; the database should be embedded so
  the project runs with `cargo run`.
- **Decision:** SQLite via `sqlx` 0.8 with the `macros`, `migrate` and
  `runtime-tokio` features; every query uses `sqlx::query!` / `query_as!`.
- **Why:** Compile-time verification catches schema drift (`query!` fails the
  build if a column disappears). Migrations are embedded in the binary with
  `sqlx::migrate!`, so deployment needs no migration step. SQLite avoids
  operating a database for a demo project.
- **Trade-offs:**
  - SQLite has a single writer. That is fine for a mailing list, but the outbox
    worker and request handlers contend on writes.
  - Live compile-time checking requires `DATABASE_URL` and a migrated database.
    CI has neither, so the **offline cache** (`.sqlx/`) is generated with
    `cargo sqlx prepare` and committed; builds then run with `SQLX_OFFLINE=true`.
    This is a small process cost that must be respected whenever a query changes
    (forget `prepare` and CI fails).

### 2.3 Subscription identity: keyed Blake3, not plaintext

- **Context:** Emails are PII. The service needs a stable lookup key and
  one-click verify/unsubscribe links.
- **Decision:** `email_hash = Blake3::new_keyed(BLAKE3_KEY).update(email)`
  stored as `BLOB`; tokens in links are the hex-encoded hash. Raw addresses are
  only stored because mail must be delivered to them.
- **Why:** A keyed hash prevents offline enumeration of a leaked database
  (without the key). Deterministic hashes make links stateless — no per-user
  token table, no extra lookups. The unique index on `email_hash` gives
  idempotency for free.
- **Trade-offs:**
  - Tokens never expire and cannot be rotated per subscription. Compromise of
    `BLAKE3_KEY` compromises all tokens at once.
  - Same token is used for verify and unsubscribe; subscribing again after
    unsubscribing reuses the same token. Acceptable for this project, but a
    production newsletter would likely add HMAC tokens with expiry and separate
    "manage preferences" tokens.
  - Guardrail: logs must never contain raw addresses. This is enforced by a
    Semgrep rule (`email-serv-no-raw-email-logging`), because a convention alone
    is easy to break.

### 2.4 Transactional outbox instead of sending inline

- **Context:** Sending SMTP mail inside `POST /api/subscribe` would make the
  request latency depend on an external server and could lose mail if the
  process crashes between the DB write and the send.
- **Decision:** Both the subscriber row and the verification email row are
  written in **one SQLite transaction** (`subscribe_and_enqueue_verification`);
  a worker sends from the outbox later.
- **Why:** The pattern decouples request handling from delivery, survives
  restarts (pending rows are picked up again), makes retries safe, and gives an
  audit trail (`status`, `retry_count`, `last_error`).
- **Trade-offs:** Delivery is eventually consistent; the queue is only as
  durable as the local database. At-least-once semantics mean the same email
  could be sent twice if the process dies *after* SMTP accepted the message but
  *before* `mark_outbox_sent` committed. True exactly-once delivery is not
  achievable with SMTP alone; production systems add an idempotency key or
  accept rare duplicates.

### 2.5 Idempotency everywhere

- **Decision:**
  - `POST /api/subscribe` returns `200 OK` for already-verified addresses and
    does not enqueue a second verification email while one is pending/allowed.
  - `GET /api/verify` queues the welcome mail only if no `welcome` outbox item
    exists for the subscriber (`has_outbox_item`, which checks any status), so
    repeated clicks or email-scanner prefetches do not spam users.
  - `email_hash` is `UNIQUE`, so `INSERT OR IGNORE` cannot duplicate rows.
- **Why:** Public endpoints are retried by clients and hit by link prefetchers.
  Making them idempotent is cheaper than deduplicating later.

### 2.6 Worker with bounded retries

- **Decision:** One tokio task polls `email_outbox` every 10 s, takes up to 50
  pending rows, renders and sends them, and:
  - marks `sent` on success,
  - retries send failures up to 3 attempts (`MAX_SEND_ATTEMPTS`), then marks
    `failed` with `last_error`,
  - marks *permanent* failures (broken JSON context, missing template)
    immediately without retrying.
- **Why:** Retrying transient SMTP errors is essential; retrying a missing
  template is pointless. `broadcast_recipients` is updated per recipient and
  `broadcasts` counters are recomputed so an operator can see progress and
  whether a campaign is done.
- **Trade-offs:** Polling adds up to 10 s latency and a fixed query load.
  A production service would likely use `LISTEN/NOTIFY` (Postgres), a real queue,
  or an in-process channel with a startup sweep. The worker is also coupled to
  the concrete `EmailService`, which makes it harder to unit test than injecting
  a `trait Sender`.

### 2.7 Tera templates with a naming convention

- **Decision:** Templates live under `templates/`, HTML bodies are `body.html`,
  subjects are the sibling `subject.txt` (with the legacy
  `verification_body.html` / `verification_subject.txt` pair supported by
  `load_subject`). Shared chrome lives in `layouts/base.html` and
  `partials/footer.html`.
- **Why:** Separating subject from body keeps email metadata out of the HTML and
  lets translators/copywriters edit plain text. Inheritance removes the footer
  (and unsubscribe link) duplication and guarantees every mail carries it.
- **Trade-offs:** Tera's glob (`templates/**/*.html`) is evaluated at startup
  relative to the working directory; the Docker image must therefore copy
  `templates/` and run from `/app`. Sysadmins cannot override templates without
  editing the image.

### 2.8 Admin broadcast API with a pre-shared key

- **Decision:** `POST /api/admin/broadcast` accepts
  `{ "template_path": "newsletters/1/body.html", "subject": "…" }` and requires
  the `x-admin-key` header to equal `ADMIN_API_KEY` (compared in constant time).
  The handler validates that the template exists, renders it once with sample
  data, creates a `broadcasts` row, and queues one outbox item per **verified**
  subscriber, each with a unique unsubscribe link. It returns `202 Accepted`
  with `{ "broadcast_id", "queued" }`.
- **Why:** Only verified subscribers may receive mail. Failing fast on an
  unknown template prevents hundreds of permanently failed rows. Binding
  recipient rows to the broadcast keeps a per-campaign audit trail.
- **Trade-offs:** A pre-shared key in a header is the simplest scheme; it has no
  rotation, scopes, or audit identity. Real systems use signed requests or an
  IdP. The handler loads all verified subscribers into memory — fine for
  thousands, not for millions.

### 2.9 Error handling with `thiserror` and JSON responses

- **Decision:** One `EmailServError` enum implements `IntoResponse`; every
  handler returns `Result<Response, EmailServError>`. Client errors map to
  `400`/`401`/`413`/`429`, server errors to `500`, and the body is always
  `{ "error": "…", "code": … }`.
- **Why:** Central mapping avoids hand-written status codes per handler and
  gives clients a machine-readable error shape. Typed errors (`EmailEmpty`,
  `Unauthorized`, `BadRequest`) keep handler code declarative via `?`.
- **Trade-offs:** The enum mixes transport concerns with domain errors; larger
  applications often split `domain::Error` from `api::Error`. Internal details
  are not hidden from clients (fine here; a production API should avoid leaking
  database strings).

### 2.10 Middleware: rate limiting and body size

- **Decision:** `tower_governor` at 10 requests/second with burst 5, keyed by
  client IP (`SmartIpKeyExtractor`, honoring `X-Forwarded-For`), plus a 2 MiB
  `RequestBodyLimitLayer`. A background task periodically calls
  `limiter.retain_recent()` so the in-memory map does not grow without bound.
- **Why:** Public subscription endpoints are abuse targets; the cleanup task is
  a small but easy-to-forget operational detail. Tests cover the burst boundary
  (7 requests → 5 OK, then `429`).
- **Trade-offs:** In-memory limiting is per process, not global; behind a proxy
  the `X-Forwarded-For` header must be trustworthy.

### 2.11 Logging: structured, file-based, PII-free

- **Decision:** `tracing` with a daily rolling JSON file appender plus an
  error-only console layer. `RUST_LOG` controls the filter.
- **Why:** JSON logs are easy to ship/query. Keeping raw email addresses out of
  logs is part of the privacy stance and is enforced by Semgrep.
- **Trade-offs:** `LOG_DIR` must exist/writable (created at startup); container
  log collection still needs a sidecar or volume to leave the host.

### 2.12 Toolchain pinning and dependency updates

- **Context:** The request was to "update crates and use the Rust LTS".
  As of October 2026 the Rust Project has **not shipped an official LTS**: the
  policy exists only as a pre-RFC, and the first designated LTS is expected to
  be **1.103 (~March 2027)**. The current stable is **1.99.0**.
- **Decision:** Pin `rust-toolchain.toml` to `1.99.0` (with `rustfmt` and
  `clippy`), declare `rust-version = "1.99"` in `Cargo.toml`, and update all
  dependencies to the newest semver-compatible versions with `cargo update`.
- **Why:** For an actively developed project, stable is the recommended
  baseline (the pre-RFC itself says "use stable unless an external conformance
  requirement forbids it"). Pinning the exact version makes CI and Docker
  reproducible; when 1.103 ships, the swap is a one-line change in
  `rust-toolchain.toml` (plus the Docker image tag).
- **Trade-offs:** Pinning to an exact version means security patch releases for
  later stables are not picked up automatically; bumping is a deliberate commit.
  The Dockerfile pins `rust:1.99-alpine` in the same way.

### 2.13 Supply-chain and static analysis: cargo-deny + Semgrep

- **Decision:**
  - `deny.toml` runs the four `cargo-deny` checks in CI: **advisories**
    (vulnerabilities and unmaintained crates), **licenses** (permissive
    allow-list; no copyleft), **bans** (wildcards denied, `dotenv` denied), and
    **sources** (crates.io only; git dependencies fail).
  - `.semgrep.yml` adds project-specific rules: no raw email addresses in
    `tracing` calls, no `unwrap()` in HTTP handlers, no `println!`/`eprintln!`,
    no `format!`-built SQL, no hardcoded secrets, no `unsafe`.
  - `.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy
    --all-targets -- -D warnings`, `cargo test`, `cargo deny check` and
    `semgrep scan --config .semgrep.yml --error`.
- **Why:** `cargo-deny` immediately paid off: it flagged `dotenv` as
  unmaintained (RUSTSEC-2021-0141), which was replaced with `dotenvy` (a
  drop-in fork) and banned in `deny.toml` so it cannot come back. Semgrep turns
  the project's security conventions into executable checks.
- **Trade-offs:**
  - `cargo-deny` license allow-lists need maintenance when dependencies change;
    `multiple-versions` only warns because transitive duplicates are usually
    harmless.
  - Semgrep's Rust support for macro arguments is limited; the raw-email rule
    uses a regex over the log call rather than a structural pattern (documented
    in the rule file). Custom rules need tuning to avoid false positives.

### 2.14 Docker image

- **Decision:** Multi-stage build on `rust:1.99-alpine` → `alpine:3.20`, musl
  static binary, non-root `app:app`, healthcheck on `/health_check`, volumes for
  `/app/data` and `/app/logs`.
- **Why:** Small attack surface and no shell/toolchain in the runtime image.
  The build copies `.sqlx/` and `migrations/` and sets `SQLX_OFFLINE=true`
  because `query!` needs either a database or the offline cache, and
  `sqlx::migrate!` embeds `migrations/` at compile time. `templates/` is copied
  into the runtime image because Tera loads it at startup.
- **Trade-offs:** `native-tls`/OpenSSL static libraries are needed at build time
  (`openssl-dev`, `openssl-libs-static`); switching lettre to its rustls backend
  would remove that. Image cannot be hot-patched with new templates (rebuild
  required).

---

## 3. Testing strategy

- 27 integration tests against the real router using
  `tower::ServiceExt::oneshot` and an in-memory SQLite database
  (`:memory:` + `run_migrations`), so no fixtures or cleanup are needed.
- HTTP behavior is tested through the router (validation, error JSON shape,
  body-size limit, fallback route).
- Security-relevant flows have dedicated tests: rate-limit burst boundary,
  idempotent subscribe/verify, broadcast only to verified subscribers, missing
  admin key returns `401`, unknown template returns `400`.
- Template rendering is tested directly against `EmailService`, including the
  welcome/newsletter/verification templates.

**Not covered (deliberately or by omission):** actual SMTP delivery (would need
a mock server such as MailHog/Mailpit or an injected `Sender` trait), the
worker loop as a unit, and multi-process rate limiting.

---

## 4. What can be learned from this project

**Rust / Tokio / Axum**

- Sharing state: `axum::extract::State` + a cheap-to-clone `ApiContext`
  containing `Clone` handles (`SqlitePool`, `Arc<Tera>`) instead of a global
  mutex.
- Middleware ordering matters: `.with_state()` after routes, `GovernorLayer`
  before the router, body limit before handlers.
- `tower::ServiceExt::oneshot` makes full-router tests fast and socket-free;
  the rate-limit test uses a real `TcpListener` only where a real socket is
  intrinsically required.
- Long-lived background tasks (`tokio::spawn`) are a clean way to run a queue
  worker, but the loop must log-and-continue on errors rather than exit.

**Data & correctness**

- Compile-time SQL (`sqlx::query!`) turns schema mistakes into build failures,
  at the cost of an offline-cache workflow (`.sqlx`, `cargo sqlx prepare`,
  `SQLX_OFFLINE=true`).
- The transactional outbox is a small pattern with a big reliability payoff:
  persist intent, process later, retry, record errors.
- Idempotency is a design property, not an afterthought; unique constraints and
  "already queued" checks make retries harmless.
- A deterministic keyed hash is enough to build stateless verify/unsubscribe
  links — and its limitations (no expiry/rotation) are worth understanding
  before copying the pattern.

**Security & operations**

- Use constant-time comparison for secrets; do not rely on `==`.
- Encode privacy rules in tooling (Semgrep) instead of trusting reviewers.
- `cargo-deny` catches unmaintained and license-incompatible dependencies early;
  supply-chain hygiene is a CI concern.
- `thiserror` + `IntoResponse` gives consistent error responses without
  per-handler `match` blocks.
- Structured logging plus "never log PII" is a habit that must be enforced;
  `tracing::info!(email)` is one keystroke away.

**Process**

- Pin toolchains (`rust-toolchain.toml`) and document *why* a version was
  chosen; "LTS" is not always available, and saying so explicitly is better
  than guessing.
- Regenerate and commit artifacts that CI needs (`.sqlx/`), and keep Docker's
  build context in sync (migrations, offline cache, templates).
- Update all checks after a feature: `cargo fmt`, `clippy -D warnings`,
  `cargo test`, `cargo deny check`, `semgrep`.

---

## 5. Known limitations & next steps

1. **SMTP testing:** add Mailpit/MailHog to `docker-compose.yml` and an
   integration test (or a fake `Sender` trait) so the send path is exercised.
2. **Token hygiene:** add expiry/rotation for verification and unsubscribe
   tokens (e.g. HMAC with a timestamp) and a documented key-rotation story.
3. **Resend verification:** a cooldown-based endpoint so users can request a new
   verification email without re-subscribing.
4. **Broadcast ergonomics:** progress endpoint
   (`GET /api/admin/broadcast/:id`), cancellation, and batching/pagination
   instead of loading all subscribers into memory.
5. **Scalability:** move to Postgres when write contention matters; use
   `NOTIFY`/a queue instead of polling; add database connection-pool limits.
6. **Deliverability:** DKIM signing, `List-Unsubscribe` headers, bounce
   handling (lettre supports adding custom headers).
7. **Observability:** metrics (sent/failed/queue depth) and alerts on
   permanently failed outbox items.
8. **Test coverage:** unit tests for `load_subject` edge cases and the
   constant-time comparison helper; property tests for `validate_email`.

---

## 6. Further reading

- [`plan.md`](plan.md) — the original implementation plan this code follows.
- [`AGENTS.md`](AGENTS.md) — build/lint/test commands and conventions.
- [`DOCKER_DEPLOYMENT.md`](DOCKER_DEPLOYMENT.md) — deployment guide.
