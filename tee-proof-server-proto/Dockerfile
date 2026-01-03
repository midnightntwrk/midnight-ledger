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

# Install runtime dependencies (curl for health checks)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1000 proofserver && \
    mkdir -p /app/data && \
    mkdir -p /home/proofserver/.cache/midnight && \
    chown -R proofserver:proofserver /app && \
    chown -R proofserver:proofserver /home/proofserver

WORKDIR /app

# Copy the built binary from builder stage
COPY --from=builder --chown=proofserver:proofserver /workspace/proof-server-binary /app/proof-server

# Set permissions
RUN chmod +x /app/proof-server

# Switch to non-root user
USER proofserver

# Pre-download ZSwap parameters by running the server with --no-fetch-params disabled
# This will download and cache all parameters, then exit immediately
RUN timeout 120 /app/proof-server --disable-tls --disable-auth || \
    (echo "Parameter download completed (server exited as expected)" && exit 0)

# Expose the proof server port
EXPOSE 6300

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -f http://localhost:6300/health || exit 1

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

# Run the proof server
# Using environment variables instead of command args for easier configuration
# TLS is disabled by default for Nitro Enclaves - use vsock proxy with TLS on parent instance
# ZSwap parameters are pre-downloaded and cached during build (no internet access needed)
CMD ["/app/proof-server"]
