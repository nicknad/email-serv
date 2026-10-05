# Build stage
FROM rust:1.99-alpine AS builder

# Build dependencies:
# - musl-dev/build-base: linking
# - pkgconf/openssl-dev/openssl-libs-static: lettre native-tls + sqlx TLS
RUN apk add --no-cache build-base musl-dev pkgconf openssl-dev openssl-libs-static

WORKDIR /app

# Manifests + SQLx offline metadata (query! macros compile without a database)
COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY migrations ./migrations

# Cache the dependency build using a dummy binary. With no src/lib.rs the
# library target is not built yet, so the SQLx macros are not expanded here.
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    SQLX_OFFLINE=true cargo build --release && \
    rm -rf src

# Real build (sqlx::migrate! embeds the migrations directory at compile time)
COPY src ./src
RUN SQLX_OFFLINE=true cargo build --release --target x86_64-unknown-linux-musl

# Runtime stage
FROM alpine:3.20

# Install runtime dependencies
RUN apk add --no-cache ca-certificates

# Create non-root user for security
RUN addgroup -g 1000 app && \
    adduser -D -u 1000 -G app app

# Set working directory
WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/email-serv /app/email-serv

# Templates are loaded at runtime (Tera), so they must ship with the image
COPY templates ./templates

# Create directories for logs and database with proper permissions
RUN mkdir -p /app/data /app/logs && \
    chown -R app:app /app

# Switch to non-root user
USER app

# Environment variables
ENV RUST_LOG=email-serv=info \
    LOG_DIR=/app/logs \
    DB_CONN=/app/data/subscribers.db \
    PORT=8080

# Expose the port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/health_check || exit 1

# Run the application
CMD ["./email-serv"]
