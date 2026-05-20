# syntax=docker/dockerfile:1.7
# Scan: docker scout cves --only-severity critical,high .
# Alt:  trivy image <image-name>

FROM node:22-bookworm-slim AS node-runtime

FROM node-runtime AS frontend-deps
WORKDIR /app/admin-ui
COPY admin-ui/package.json admin-ui/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci

FROM rust:1.87-bookworm AS builder
WORKDIR /app

COPY --from=node-runtime /usr/local/bin/node /usr/local/bin/node
COPY --from=node-runtime /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -sf /usr/local/lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf /usr/local/lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx

COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY admin-ui ./admin-ui
COPY --from=frontend-deps /app/admin-ui/node_modules ./admin-ui/node_modules

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --locked --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gosu tini \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system --gid 10001 floral \
    && useradd --system --uid 10001 --gid floral --home-dir /var/lib/floral-sync --create-home --shell /usr/sbin/nologin floral

WORKDIR /var/lib/floral-sync

COPY --from=builder /app/target/release/floral-sync-server /usr/local/bin/floral-sync-server
COPY docker/entrypoint.sh /usr/local/bin/docker-entrypoint.sh
COPY docker/healthcheck.sh /usr/local/bin/docker-healthcheck.sh

RUN chmod 0755 /usr/local/bin/floral-sync-server /usr/local/bin/docker-entrypoint.sh /usr/local/bin/docker-healthcheck.sh \
    && mkdir -p /var/lib/floral-sync/config /var/lib/floral-sync/data /var/lib/floral-sync/exports /var/lib/floral-sync/logs \
    && chown -R floral:floral /var/lib/floral-sync

ENV FLORAL_SYNC_LISTEN=0.0.0.0:8787 \
    FLORAL_ADMIN_LISTEN=0.0.0.0:8788 \
    FLORAL_CONFIG_PATH=/var/lib/floral-sync/config/sync-server.toml \
    FLORAL_DB_PATH=/var/lib/floral-sync/data/floral-sync.sqlite3 \
    FLORAL_EXPORT_DIR=/var/lib/floral-sync/exports \
    FLORAL_LOG_PATH=/var/lib/floral-sync/logs/floral-sync-server.log \
    FLORAL_LOG_LEVEL=info

EXPOSE 8787 8788

VOLUME ["/var/lib/floral-sync/config", "/var/lib/floral-sync/data", "/var/lib/floral-sync/exports", "/var/lib/floral-sync/logs"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/docker-healthcheck.sh"]

LABEL org.opencontainers.image.title="floral-sync-server" \
    org.opencontainers.image.security.capabilities.drop="ALL"

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/docker-entrypoint.sh"]