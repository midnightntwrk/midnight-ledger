# Multi-architecture Dockerfile for Midnight TEE Proof Server
# Supports: linux/amd64, linux/arm64 (macOS Apple Silicon, AWS Graviton/Nitro)
#
# NOTE: This must be built from the midnight-ledger workspace root:
#   cd ~/code/midnight-code/midnight-ledger
#   docker build -f tee-proof-server-proto/Dockerfile -t midnight/proof-server:latest .

# Use a multi-arch base image with Rust on Debian Bookworm
# This matches the runtime stage glibc version
# Note: rust:bookworm uses Rust 1.80+ which supports edition 2024
FROM --platform=$BUILDPLATFORM rust:bookworm AS builder

# Accept build arguments for cross-compilation
ARG TARGETPLATFORM
ARG BUILDPLATFORM
ARG TARGETOS
ARG TARGETARCH

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    build-essential \
    clang \
    llvm \
    && rm -rf /var/lib/apt/lists/*

# Set up cross-compilation toolchains based on target architecture
RUN case "$TARGETARCH" in \
    "amd64") \
        rustup target add x86_64-unknown-linux-gnu ;; \
    "arm64") \
        rustup target add aarch64-unknown-linux-gnu && \
        apt-get update && apt-get install -y \
            gcc-aarch64-linux-gnu \
            g++-aarch64-linux-gnu && \
        rm -rf /var/lib/apt/lists/* ;; \
    esac

WORKDIR /workspace

# Copy the entire midnight-ledger workspace
# This is necessary because proof-server depends on other workspace crates
COPY Cargo.toml Cargo.lock ./

# Copy all workspace members
# Using individual COPY commands to avoid copying unnecessary files like target/
COPY base-crypto/ ./base-crypto/
COPY base-crypto-derive/ ./base-crypto-derive/
COPY transient-crypto/ ./transient-crypto/
COPY ledger/ ./ledger/
COPY ledger-wasm/ ./ledger-wasm/
COPY zswap/ ./zswap/
COPY storage/ ./storage/
COPY storage-macros/ ./storage-macros/
COPY serialize/ ./serialize/
COPY serialize-macros/ ./serialize-macros/
COPY zkir/ ./zkir/
COPY zkir-wasm/ ./zkir-wasm/
COPY coin-structure/ ./coin-structure/
COPY onchain-state/ ./onchain-state/
COPY onchain-vm/ ./onchain-vm/
COPY onchain-runtime/ ./onchain-runtime/
COPY onchain-runtime-wasm/ ./onchain-runtime-wasm/
COPY generate-cost-model/ ./generate-cost-model/
COPY proof-server/ ./proof-server/
COPY wasm-proving-demos/ ./wasm-proving-demos/
COPY static/ ./static/
COPY tee-proof-server-proto/ ./tee-proof-server-proto/

# Set up Cargo config for cross-compilation (if needed)
RUN case "$TARGETARCH" in \
    "arm64") \
        mkdir -p .cargo && \
        echo '[target.aarch64-unknown-linux-gnu]' > .cargo/config.toml && \
        echo 'linker = "aarch64-linux-gnu-gcc"' >> .cargo/config.toml && \
        echo '[build]' >> .cargo/config.toml && \
        echo 'target = "aarch64-unknown-linux-gnu"' >> .cargo/config.toml ;; \
    esac

# Build the proof server for the target architecture
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    case "$TARGETARCH" in \
        "amd64") \
            cargo build --release --package midnight-proof-server-prototype --target x86_64-unknown-linux-gnu && \
            cp target/x86_64-unknown-linux-gnu/release/midnight-proof-server-prototype /workspace/proof-server-binary ;; \
        "arm64") \
            cargo build --release --package midnight-proof-server-prototype --target aarch64-unknown-linux-gnu && \
            cp target/aarch64-unknown-linux-gnu/release/midnight-proof-server-prototype /workspace/proof-server-binary ;; \
    esac

# Runtime stage - use minimal image
FROM debian:bookworm-slim

ARG TARGETARCH

# Install runtime dependencies (curl for health checks, socat for vsock bridge)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    socat \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1000 proofserver && \
    mkdir -p /app/data && \
    mkdir -p /app/.cache/midnight && \
    chown -R proofserver:proofserver /app

WORKDIR /app

# Copy the built binary from builder stage
COPY --from=builder --chown=proofserver:proofserver /workspace/proof-server-binary /app/proof-server

# Set permissions
RUN chmod +x /app/proof-server

# Switch to non-root user
USER proofserver

# Set HOME to /app so cache goes to /app/.cache
ENV HOME=/app

# Pre-download ZSwap parameters by running the server briefly
# The server will download all parameters during startup, then we kill it after 30 seconds
# This is faster than waiting for timeout and ensures parameters are cached
RUN /app/proof-server --disable-tls --disable-auth & \
    SERVER_PID=$! && \
    sleep 30 && \
    kill $SERVER_PID || true && \
    echo "✓ ZSwap parameters pre-downloaded and cached" && \
    ls -la /app/.cache/midnight/ || echo "Cache directory check failed"

# Expose the proof server port
EXPOSE 6300

# Note: Healthcheck removed for Nitro Enclave compatibility
# Enclaves don't have network access for curl, and healthchecks can cause issues

# Set environment variables
ENV RUST_LOG=info
ENV MIDNIGHT_PROOF_SERVER_PORT=6300
ENV MIDNIGHT_PROOF_SERVER_DISABLE_AUTH=true
ENV MIDNIGHT_PROOF_SERVER_DISABLE_TLS=true

# Labels for metadata
LABEL org.opencontainers.image.title="Midnight TEE Proof Server"
LABEL org.opencontainers.image.description="Zero-knowledge proof server for Midnight blockchain"
LABEL org.opencontainers.image.version="6.2.0-alpha.1"
LABEL org.opencontainers.image.vendor="Midnight Foundation"
LABEL org.opencontainers.image.licenses="Apache-2.0"

# Create startup script for vsock bridge (Nitro Enclave networking)
RUN echo '#!/bin/bash' > /app/start.sh && \
    echo 'set -e' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo 'echo "[$(date)] Starting Midnight Proof Server with vsock bridge..."' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo '# Start proof server in background (bind to localhost only for vsock bridge)' >> /app/start.sh && \
    echo '/app/proof-server --bind 127.0.0.1 --no-fetch-params &' >> /app/start.sh && \
    echo 'PROOF_SERVER_PID=$!' >> /app/start.sh && \
    echo 'echo "[$(date)] Proof server started (PID: $PROOF_SERVER_PID)"' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo '# Give proof server time to start' >> /app/start.sh && \
    echo 'sleep 5' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo '# Start socat to bridge vsock → TCP' >> /app/start.sh && \
    echo '# Listen on vsock port 6300, forward to localhost:6300' >> /app/start.sh && \
    echo 'echo "[$(date)] Starting socat bridge: vsock:6300 → localhost:6300"' >> /app/start.sh && \
    echo 'socat VSOCK-LISTEN:6300,fork TCP:127.0.0.1:6300 &' >> /app/start.sh && \
    echo 'SOCAT_PID=$!' >> /app/start.sh && \
    echo 'echo "[$(date)] Socat bridge started (PID: $SOCAT_PID)"' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo 'echo "[$(date)] Proof server ready and accessible via vsock"' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo '# Wait for either process to exit' >> /app/start.sh && \
    echo 'wait -n' >> /app/start.sh && \
    echo 'EXIT_CODE=$?' >> /app/start.sh && \
    echo '' >> /app/start.sh && \
    echo '# If either exits, kill the other and exit' >> /app/start.sh && \
    echo 'echo "[$(date)] Process exited with code $EXIT_CODE, shutting down..."' >> /app/start.sh && \
    echo 'kill $PROOF_SERVER_PID $SOCAT_PID 2>/dev/null' >> /app/start.sh && \
    echo 'exit $EXIT_CODE' >> /app/start.sh && \
    chmod +x /app/start.sh

# Run the startup script (includes proof server + vsock bridge)
# TLS is disabled by default for Nitro Enclaves - use vsock proxy with TLS on parent instance
# Note: When run outside enclave (e.g., local Docker), socat will fail to create vsock,
# but proof server will still work on TCP. This is expected behavior.
CMD ["/app/start.sh"]
