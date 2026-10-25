# ==============================================================================
# evaNotes — Multi-Stage Production Dockerfile
# ==============================================================================

# --- Stage 1: Build release binary ---
FROM rust:1-slim-bookworm AS builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Copy migrations (embedded at compile-time by refinery)
COPY migrations ./migrations

# Copy embedded font assets (required by include_bytes! in services/pdf.rs)
COPY assets ./assets

# Copy application source code
COPY src ./src

# Build production release binary (--locked guarantees reproducible deps)
RUN cargo build --release --locked --bin evanotes

# --- Stage 2: Minimal hardened runtime ---
FROM debian:bookworm-slim AS runtime

# Install CA certificates and curl for healthchecks
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root unprivileged service user
RUN groupadd -g 1000 evanotes && \
    useradd -u 1000 -g evanotes -s /bin/sh -m evanotes

WORKDIR /app

# Prepare default database storage directory with appropriate ownership
RUN mkdir -p /data && chown -R evanotes:evanotes /data /app

# Copy compiled binary from builder
COPY --from=builder /usr/src/app/target/release/evanotes /usr/local/bin/evanotes

# Copy static frontend assets
COPY --from=builder /usr/src/app/src/web/static ./src/web/static

# Embed the DejaVu font license notice (required for redistribution)
COPY assets/fonts/LICENSE /usr/share/doc/evanotes/fonts-LICENSE

# Switch to unprivileged user
USER evanotes

# Default runtime configuration
ENV EVA_DATA_DIR=/data \
    HOST=0.0.0.0 \
    PORT=8080 \
    RUST_LOG=info

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/api/auth/status || exit 1

ENTRYPOINT ["/usr/local/bin/evanotes"]
