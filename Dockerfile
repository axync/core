# Multi-stage build for optimized image size
FROM rust:latest AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libclang-dev \
    clang \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency files for caching
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build dependencies and project
# Use release profile for optimization
RUN cargo build --release -p zkclear-api --features rocksdb

# Final image
# Use Ubuntu 24.04 which has GLIBC 2.39 (compatible with rust:latest builds)
FROM ubuntu:24.04

# Install required libraries for RocksDB and runtime
RUN apt-get update && apt-get install -y \
    libgcc-s1 \
    libc6 \
    libstdc++6 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
# Remove existing user with UID 1000 if it exists, then create zkclear user
RUN if getent passwd 1000 > /dev/null 2>&1; then \
        EXISTING_USER=$(getent passwd 1000 | cut -d: -f1); \
        userdel -r "$EXISTING_USER" 2>/dev/null || true; \
    fi && \
    useradd -m -u 1000 zkclear && \
    mkdir -p /app/data && \
    chown -R zkclear:zkclear /app

WORKDIR /app

# Copy compiled binary
COPY --from=builder /app/target/release/zkclear-api /app/zkclear-api

# Set ownership
RUN chown zkclear:zkclear /app/zkclear-api && \
    chmod +x /app/zkclear-api

# Switch to non-root user
USER zkclear

# Environment variables
ENV RUST_LOG=info
ENV DATA_DIR=/app/data
ENV STORAGE_PATH=/app/data
ENV BLOCK_INTERVAL_SEC=1
ENV MAX_QUEUE_SIZE=10000
ENV MAX_TXS_PER_BLOCK=100

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Expose port
EXPOSE 8080

# Run application
CMD ["./zkclear-api"]

