# Debian 11 provides the glibc 2.31 sysroot. Use the archive mirror so this
# build remains reproducible after bullseye leaves the normal mirror, while
# apt.llvm.org provides architecture-matched LLVM 22 development packages.
ARG OLD_GLIBC_IMAGE=debian:bullseye-slim@sha256:f313b4bd62667092a59b3a664d7d3ab8b5e65f41675f48e81455a15dc5abe792
FROM ${OLD_GLIBC_IMAGE}

# The archived slim image has no CA bundle. Debian Release signatures are still
# checked while bootstrapping ca-certificates; only TLS peer validation is
# disabled for this first signed archive fetch.
#
# `bullseye-security` comes from snapshot.debian.org, pinned to a timestamp.
# Bullseye is EOL and Debian is retiring it, which broke this image three times
# in four days:
#
#   run 34197616242 — E: Release file for .../bullseye-security/InRelease is
#     expired (Valid-Until was Mon, 07 Sep 2026 21:13:04 UTC).
#   run 34272956353 — with check-valid-until=no, the same suite began returning
#     404 for its .debs from some Fastly nodes (151.101.74.132) while serving
#     200 from others. A CDN lottery.
#   run 34293996179 — dropping the suite entirely then broke apt's resolver:
#     the PINNED BASE IMAGE already carries security versions, so archive-only
#     sources cannot satisfy them —
#       libc6-dev : Depends: libc6 (= 2.31-13+deb11u11) but ...u14 is installed
#       libssl-dev: Depends: libssl1.1 (= 1.1.1w-0+deb11u1) but ...u8 is installed
#       perl      : Depends: perl-base (= 5.32.1-4+deb11u3) but ...u5 is installed
#
# archive.debian.org does NOT carry debian-security (404), so the only stable
# source of those exact versions is snapshot.debian.org — Debian's timestamped
# time-machine, immutable by design, immune to both expiry and CDN state.
# Verified at 20260901T000000Z: libc6 2.31-13+deb11u14, libssl1.1 1.1.1w-0+deb11u8,
# perl-base 5.32.1-4+deb11u5, gpgv 2.2.27-2+deb11u3 — exactly what the pinned
# base image has installed.
#
# The timestamp is part of the reproducibility contract: bump it only alongside
# a base-image digest bump, and re-check those four versions when you do.

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
#
# Two extra safeguards, both learned from run 34314310247:
#
#   * APT PIN. Debian's bullseye-security genuinely ships LLVM 22 packages
#     (clang-22, libpolly-22-dev, …), so apt preferred snapshot's copies over
#     apt.llvm.org and tried to pull the LARGE LLVM debs through snapshot —
#     which is an archival service, not a throughput mirror, and reset the
#     connection: "Failed to fetch .../libpolly-22-dev_22.1.8-1~deb11u1_amd64.deb
#     Error reading from server. Remote end closed connection". Pinning
#     origin apt.llvm.org at 1001 keeps the bulk on the fast mirror and leaves
#     snapshot serving only the four small base packages it is needed for.
#     `Package: *` on purpose, NOT a glob: a first attempt listed
#     `clang-* llvm-* libclang-* ...` and MISSED libllvm22 (no hyphen) and
#     libclang1-22 (libclang1-, not libclang-), so those two resolved to
#     Debian's 1:22.1.8-1~deb11u1 while clang-22 came from apt.llvm.org's
#     1:22.1.8~++2026...  — versions that cannot satisfy each other. Scoping by
#     ORIGIN rather than by name is exhaustive by construction; apt.llvm.org
#     only publishes LLVM packages, so a wildcard here is safe.
#   * Acquire::Retries=5, because snapshot drops connections under load.
RUN printf '%s\n' \
      'deb [check-valid-until=no] https://archive.debian.org/debian bullseye main' \
      'deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/20260901T000000Z bullseye-security main' \
      > /etc/apt/sources.list \
    && apt-get -o Acquire::https::Verify-Peer=false -o Acquire::Retries=5 update \
    && DEBIAN_FRONTEND=noninteractive apt-get \
      -o Acquire::https::Verify-Peer=false -o Acquire::Retries=5 \
      install -y --no-install-recommends ca-certificates \
    && apt-get -o Acquire::Retries=5 update \
    && DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=5 install -y --no-install-recommends \
      build-essential cmake curl gnupg libssl-dev libzstd-dev \
      perl pkg-config xz-utils zlib1g-dev \
    && curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
      -o /etc/apt/trusted.gpg.d/apt.llvm.org.asc \
    && printf '%s\n' \
      'deb https://apt.llvm.org/bullseye/ llvm-toolchain-bullseye-22 main' \
      > /etc/apt/sources.list.d/llvm22.list \
    && printf '%s\n' \
      'Package: *' \
      'Pin: origin apt.llvm.org' \
      'Pin-Priority: 1001' \
      > /etc/apt/preferences.d/llvm-from-upstream \
    && apt-get -o Acquire::Retries=5 update \
    && DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=5 install -y --no-install-recommends \
      clang-22 libpolly-22-dev llvm-22-dev \
    && rm -rf /var/lib/apt/lists/*

ENV LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
