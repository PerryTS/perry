# Debian 11 provides the glibc 2.31 sysroot. Use the archive mirror so this
# build remains reproducible after bullseye leaves the normal mirror, while
# apt.llvm.org provides architecture-matched LLVM 22 development packages.
ARG OLD_GLIBC_IMAGE=debian:bullseye-slim@sha256:f313b4bd62667092a59b3a664d7d3ab8b5e65f41675f48e81455a15dc5abe792
FROM ${OLD_GLIBC_IMAGE}

# The archived slim image has no CA bundle. Debian Release signatures are still
# checked while bootstrapping ca-certificates; only TLS peer validation is
# disabled for this first signed archive fetch.
#
# The `bullseye-security` suite is deliberately NOT listed. Bullseye is EOL and
# Debian is actively retiring it, which broke this image twice in four days:
#
#   run 34197616242 — E: Release file for .../bullseye-security/InRelease is
#     expired (invalid since 14h 44min 50s). Its Release carried
#     `Valid-Until: Mon, 07 Sep 2026 21:13:04 UTC`.
#   run 34272956353 — after adding `check-valid-until=no`, the same suite began
#     returning 404 for its .debs from some Fastly nodes (IP 151.101.74.132)
#     while serving 200 from others. A CDN lottery, not a clean removal.
#
# There is no archive fallback: archive.debian.org carries bullseye,
# -backports, -proposed-updates and -updates, but NOT debian-security (404).
#
# So take everything from the archive instead. Verified against
# archive.debian.org/debian/dists/bullseye/main/binary-arm64/Packages: every
# package this image installs is present there — build-essential 12.9,
# cmake 3.18.4-2+deb11u1, curl 7.74.0-1.3+deb11u13, gnupg 2.2.27-2+deb11u2,
# libssl-dev 1.1.1w-0+deb11u1, libzstd-dev 1.4.8+dfsg-2.1, perl 5.32.1-4+deb11u3,
# pkg-config 0.29.2-1, xz-utils 5.2.5-2.1~deb11u1, zlib1g-dev 1.2.11.dfsg-2+deb11u2,
# ca-certificates 20210119.
#
# The trade-off is explicit: these are the archived versions, without later
# security patches. That is acceptable for a BUILD toolchain image whose only
# job is to link against glibc 2.31 — it ships no runtime surface itself — and
# it is the standard configuration for an EOL Debian base.
RUN printf '%s\n' \
      'deb [check-valid-until=no] https://archive.debian.org/debian bullseye main' \
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
