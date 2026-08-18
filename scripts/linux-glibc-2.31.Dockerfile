# Debian 11 provides the glibc 2.31 sysroot. Use the archive mirror so this
# build remains reproducible after bullseye leaves the normal mirror, while
# apt.llvm.org provides architecture-matched LLVM 22 development packages.
ARG OLD_GLIBC_IMAGE=debian:bullseye-slim@sha256:f313b4bd62667092a59b3a664d7d3ab8b5e65f41675f48e81455a15dc5abe792
FROM ${OLD_GLIBC_IMAGE}

# The archived slim image has no CA bundle. Debian Release signatures are still
# checked while bootstrapping ca-certificates; only TLS peer validation is
# disabled for this first signed archive fetch.
RUN printf '%s\n' \
      'deb [check-valid-until=no] https://archive.debian.org/debian bullseye main' \
      'deb https://deb.debian.org/debian-security bullseye-security main' \
      > /etc/apt/sources.list \
    && apt-get -o Acquire::https::Verify-Peer=false update \
    && DEBIAN_FRONTEND=noninteractive apt-get \
      -o Acquire::https::Verify-Peer=false \
      install -y --no-install-recommends ca-certificates \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential cmake curl gnupg libssl-dev libzstd-dev \
      perl pkg-config xz-utils zlib1g-dev \
    && curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
      -o /etc/apt/trusted.gpg.d/apt.llvm.org.asc \
    && printf '%s\n' \
      'deb https://apt.llvm.org/bullseye/ llvm-toolchain-bullseye-22 main' \
      > /etc/apt/sources.list.d/llvm22.list \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      clang-22 libpolly-22-dev llvm-22-dev \
    && rm -rf /var/lib/apt/lists/*

ENV LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
