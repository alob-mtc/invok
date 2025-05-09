# === Stage 1: Build the application ===
FROM rust:1.85 AS builder

# Set the working directory to the workspace root
WORKDIR /usr/src/app

# Now copy the entire workspace source code
COPY . .

# Pre-fetch dependencies (improves caching)
RUN cargo fetch


# Build only the "serverless_core" package in release mode
RUN cargo build --release -p serverless_core

# === Stage 2: Create a minimal runtime image ===
FROM debian:bookworm-slim

# Install CA certificates and OpenSSL 3 (provides libssl.so.3)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    docker.io \
 && rm -rf /var/lib/apt/lists/*

# Create a non-root user 'appuser' and add them to the 'daemon' group.
# The 'daemon' group typically has GID 1 on many systems.
RUN useradd -m -G daemon appuser

# add to user
RUN groupadd -f docker && usermod -aG docker appuser

# Set working directory
WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/app/target/release/serverless-core /usr/local/bin

# Ensure the binary is executable
RUN chmod +x /usr/local/bin/serverless-core

# Switch to non-root user
USER appuser

# Expose the port your API listens on (adjust if necessary)
EXPOSE 3000

# Start the API application
CMD ["serverless-core"]