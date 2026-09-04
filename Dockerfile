# syntax=docker/dockerfile:1
#
# Reproducible build of the `sherwood` binary + the dashboard bundle.
#
#   docker build -t sherwood-agent .
#   docker run --rm sherwood-agent --help
#
# The server binds loopback only (see docs/SECURITY.md), so `serve` in a
# container needs `--network host` (Linux) — see docs/DEPLOYMENT.md.

# ---- Rust build ---------------------------------------------------------------
FROM rust:1.83-slim-bookworm AS rust-build
WORKDIR /src
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential \
 && rm -rf /var/lib/apt/lists/*
# sqlx query macros check against the committed .sqlx/ cache — no DB needed.
ENV SQLX_OFFLINE=true
COPY . .
RUN cargo build --release --locked -p sherwood-cli \
 && strip target/release/sherwood

# ---- Dashboard build --------------------------------------------------------
FROM node:20-slim AS web-build
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---- Runtime --------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /app --shell /usr/sbin/nologin sherwood \
 && mkdir -p /app/data /app/logs \
 && chown -R sherwood:sherwood /app
WORKDIR /app
COPY --from=rust-build /src/target/release/sherwood /usr/local/bin/sherwood
COPY --from=web-build  /web/dist                    /app/frontend/dist
COPY config.example.toml                            /app/config.example.toml

USER sherwood
ENV SHERWOOD_LOG_DIR=/app/logs
EXPOSE 8787
ENTRYPOINT ["sherwood"]
CMD ["serve", "/app/config.toml"]
