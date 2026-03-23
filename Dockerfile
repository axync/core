# Build stage
FROM rust:1.82-slim-bookworm AS builder

WORKDIR /app

# Install build deps
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy manifests first for cache
COPY Cargo.toml Cargo.lock ./
COPY crates/types/Cargo.toml crates/types/
COPY crates/state/Cargo.toml crates/state/
COPY crates/stf/Cargo.toml crates/stf/
COPY crates/storage/Cargo.toml crates/storage/
COPY crates/prover/Cargo.toml crates/prover/
COPY crates/sequencer/Cargo.toml crates/sequencer/
COPY crates/watcher/Cargo.toml crates/watcher/
COPY crates/api/Cargo.toml crates/api/
COPY crates/demo/Cargo.toml crates/demo/

# Create dummy src files for dep cache
RUN mkdir -p crates/types/src crates/state/src crates/stf/src crates/storage/src \
    crates/prover/src crates/sequencer/src crates/watcher/src crates/api/src crates/demo/src && \
    for d in types state stf storage prover sequencer watcher api demo; do echo "" > crates/$d/src/lib.rs; done && \
    echo "fn main() {}" > crates/api/src/main.rs && \
    echo "fn main() {}" > crates/demo/src/main.rs

RUN cargo build --release --bin axync-api 2>/dev/null || true

# Copy real source
COPY crates/ crates/
RUN touch crates/*/src/*.rs

RUN cargo build --release --bin axync-api

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/axync-api .

RUN mkdir -p data

EXPOSE 8080

CMD ["./axync-api"]
