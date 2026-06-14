# ── Stage 1: build the React / Vite frontend ────────────────────────────────
FROM node:22-alpine AS web-builder
WORKDIR /app/web

COPY web/package.json web/package-lock.json* ./
RUN npm install

COPY web/ ./
RUN npm run build


# ── Stage 2: build the Rust binary ──────────────────────────────────────────
FROM rust:slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests and lock file first so cargo can cache the dep-fetch layer.
COPY Cargo.toml Cargo.lock build.rs ./

# Bring in pre-built web dist so rust-embed can embed it.
COPY --from=web-builder /app/web/dist ./web/dist

# Warm the dependency cache with a stub main (rebuild triggered by src changes).
RUN mkdir -p src && printf 'fn main(){}' > src/main.rs
ENV SKIP_WEB_BUILD=1
RUN cargo build --release 2>/dev/null; rm -f src/main.rs

# Now compile the real source.
COPY src/ src/
COPY migrations/ migrations/
RUN touch src/main.rs && cargo build --release


# ── Stage 3: minimal runtime image ──────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/gust /usr/local/bin/gust

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/gust"]
