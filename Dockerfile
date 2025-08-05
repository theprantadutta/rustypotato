# Multi-stage build for optimized production image
FROM rust:1.88-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code and benchmarks (needed for Cargo.toml validation)
COPY src ./src
COPY benches ./benches

# Build for release (exclude benchmarks and tests for Docker)
RUN cargo build --release --bins

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create rustypotato user
RUN groupadd -r rustypotato && useradd -r -g rustypotato rustypotato

# Create directories
RUN mkdir -p /data /etc/rustypotato /var/log/rustypotato && \
    chown -R rustypotato:rustypotato /data /etc/rustypotato /var/log/rustypotato

# Copy binaries from builder stage
COPY --from=builder /app/target/release/rustypotato-server /usr/local/bin/
COPY --from=builder /app/target/release/rustypotato-cli /usr/local/bin/

# Copy default configuration
COPY docker/rustypotato.toml /etc/rustypotato/rustypotato.toml

# Set permissions
RUN chmod +x /usr/local/bin/rustypotato-server /usr/local/bin/rustypotato-cli

# Switch to non-root user
USER rustypotato

# Expose ports
EXPOSE 6379 9090

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD rustypotato-cli ping || exit 1

# Default command
CMD ["rustypotato-server", "--config", "/etc/rustypotato/rustypotato.toml"]