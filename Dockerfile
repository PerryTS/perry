# Perry TypeScript Compiler - Multi-stage Docker Build
#
# This Dockerfile builds Perry and can be used to:
# 1. Build Perry from source on Linux
# 2. Compile TypeScript files to native Linux executables
# 3. Create minimal runtime images with compiled binaries
#
# Usage:
#   docker build -t perry .
#   docker run -v $(pwd):/app perry /app/myfile.ts -o /app/myfile

# =============================================================================
# Stage 1: Build Perry compiler
# =============================================================================
FROM rust:1.99-bookworm AS builder

WORKDIR /perry

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo files first for dependency caching
COPY rust-toolchain.toml ./
RUN rustup show active-toolchain
COPY .cargo/config.toml .cargo/config.toml
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY scripts ./scripts

# Build release binaries. The runtime archive comes from the -static wrapper
# crate; its GC effects sidecar is computed from the archive bytes and ships
# beside it (perry reads it at compile time).
RUN cargo build --release
RUN cargo build --release -p perry-runtime-static
RUN scripts/gc_call_effects/sidecars.sh target/release

# =============================================================================
# Stage 2: Runtime image for compiling TypeScript
# =============================================================================
FROM debian:bookworm-slim AS compiler

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /perry

# Copy Perry compiler and runtime library
COPY --from=builder /perry/target/release/perry /usr/local/bin/perry
COPY --from=builder /perry/target/release/libperry_runtime.a /usr/local/lib/libperry_runtime.a
COPY --from=builder /perry/target/release/libperry_runtime.a.gc-effects /usr/local/lib/libperry_runtime.a.gc-effects

# Set library path for linking
ENV PERRY_RUNTIME_LIB=/usr/local/lib/libperry_runtime.a

ENTRYPOINT ["perry"]

# =============================================================================
# Stage 3: Minimal image for running compiled binaries only
# =============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# This stage is meant to be used with COPY --from to add your compiled binary
# Example in your Dockerfile:
#   FROM perry:runtime
#   COPY --from=builder /app/myprogram /app/myprogram
#   CMD ["/app/myprogram"]
