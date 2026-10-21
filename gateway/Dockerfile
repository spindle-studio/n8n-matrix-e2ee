FROM rust:1.95-bookworm AS builder
RUN apt-get update \
    && apt-get install --no-install-recommends -y libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --locked --release --package n8n-matrix-e2ee-gateway

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --shell /usr/sbin/nologin appuser \
    && install -d --owner=10001 --group=10001 /app /app/data /data
WORKDIR /app
COPY --from=builder /app/target/release/n8n-matrix-e2ee-gateway /usr/local/bin/n8n-matrix-e2ee-gateway
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/n8n-matrix-e2ee-gateway"]
