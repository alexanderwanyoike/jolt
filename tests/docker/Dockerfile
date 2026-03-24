# Multi-stage build for dweb binary
FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --bin dweb

# Runtime image with networking tools
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    curl \
    jq \
    iproute2 \
    iptables \
    procps \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/dweb /usr/local/bin/dweb
COPY tests/docker/node-entrypoint.sh /usr/local/bin/node-entrypoint.sh
RUN chmod +x /usr/local/bin/node-entrypoint.sh

# Create data directory
RUN mkdir -p /root/.dweb

ENTRYPOINT ["dweb"]
