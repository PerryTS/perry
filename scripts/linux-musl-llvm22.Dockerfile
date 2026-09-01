# musl build sysroot for the Linux musl release legs.
#
# Why a container at all: perry links libLLVM in-process (default since
# 2026-08-17), so a musl target needs a *musl-built* LLVM. The Ubuntu runners
# only have apt.llvm.org's glibc build, and linking that into a static musl
# binary fails at the very end of the build with
#   /lib/x86_64-linux-gnu/ld-linux-x86-64.so.2: DSO missing from command line
# (aarch64: libgcc_s.so.1). LLVM publishes no musl binaries.
#
# Alpine is musl-native and packages LLVM 22.1.8 — the exact version llvm-sys
# 221 requires — for both x86_64 and aarch64, so nothing is built from source.
# Mirrors the glibc-2.31 leg's container pattern.
#
# `edge` is required: llvm22 is not in a tagged Alpine release yet. Pinned by
# digest so the sysroot is reproducible; bump deliberately.
ARG ALPINE_IMAGE=alpine:edge
FROM ${ALPINE_IMAGE}

# `rust`/`cargo` from Alpine are NOT used: perry pins nightly-2026-08-20
# (rust-toolchain.toml) for `float_algebraic` and cargo's `min-publish-age`.
# rustup ships musl-host toolchains for both arches, so install through it.
RUN apk add --no-cache \
      build-base clang22 cmake curl git \
      llvm22 llvm22-dev llvm22-static llvm22-libs \
      libffi-dev openssl-dev zlib-dev zstd-dev xz-dev \
      zlib-static zstd-static libxml2-static ncurses-static clang22-libclang \
      musl-dev pkgconf perl python3 \
  # Alpine installs LLVM 22 under /usr/lib/llvm22 and does NOT put its
  # llvm-config on PATH (the bare name is unversioned and absent), so the
  # check has to name the prefix explicitly.
  && /usr/lib/llvm22/bin/llvm-config --version | grep -q '^22\.' \
     || { echo "alpine llvm is not 22.x ($(/usr/lib/llvm22/bin/llvm-config --version 2>&1))" >&2; exit 1; }

# llvm-sys links LLVM statically, so the static system libs it pulls in must
# exist here as `.a`. Alpine's `-dev` packages carry only the shared object, so
# a missing `-static` package surfaces ~20 minutes into the cargo build as
# `could not find native static library 'z'` rather than at image-build time.
#
# `llvm-config --system-libs` is EMPTY on this image (Alpine builds LLVM so it
# reports no system libs), so iterating it alone is a vacuous check that passes
# having verified nothing -- llvm-sys still demands `z`, which is what broke the
# musl legs in the first place. So: verify an explicit REQUIRED floor, and union
# it with whatever llvm-config does report.
RUN set -eu; \
    REQUIRED="z zstd"; \
    libdirs="$(/usr/lib/llvm22/bin/llvm-config --libdir) /usr/lib /usr/lib/gcc"; \
    reported="$(/usr/lib/llvm22/bin/llvm-config --system-libs || true)"; \
    names="$REQUIRED"; \
    for flag in $reported; do \
      name="${flag#-l}"; \
      [ "$name" != "$flag" ] || continue; \
      case " c m rt dl pthread util xnet " in *" $name "*) continue ;; esac; \
      names="$names $name"; \
    done; \
    checked=0; missing=""; \
    for name in $names; do \
      found=""; \
      for d in $libdirs; do \
        if [ -e "$d/lib$name.a" ]; then found=1; break; fi; \
      done; \
      checked=$((checked + 1)); \
      [ -n "$found" ] || missing="$missing $name"; \
    done; \
    if [ -n "$missing" ]; then \
      echo "missing static system libs for llvm-sys:$missing" >&2; \
      echo "(llvm-config --system-libs: '$reported')" >&2; \
      exit 1; \
    fi; \
    if [ "$checked" -lt 2 ]; then \
      echo "static-libs check verified only $checked lib(s) -- it is not testing anything" >&2; \
      exit 1; \
    fi; \
    echo "static system libs OK: checked $checked ($names)"

ENV LLVM_SYS_221_PREFIX=/usr/lib/llvm22
ENV RUSTUP_HOME=/opt/rustup
ENV CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:/usr/lib/llvm22/bin:$PATH

# `libsqlite3-sys` runs bindgen, which dlopens libclang at BUILD time. Alpine
# ships the shared library in `clang22-libclang` (plain `clang22` is the driver
# only) and does not put it on the default search path, so bindgen needs both
# the package and this variable. Declared AFTER the other ENVs and asserted
# below, so an Alpine layout change fails here instead of ~8 minutes into the
# cargo build with "Unable to find libclang".
ENV LIBCLANG_PATH=/usr/lib/llvm22/lib
RUN set -eu; \
    for c in "$LIBCLANG_PATH"/libclang.so "$LIBCLANG_PATH"/libclang.so.*; do \
      if [ -e "$c" ]; then echo "libclang OK: $c"; exit 0; fi; \
    done; \
    echo "no libclang.so under $LIBCLANG_PATH (bindgen would fail)" >&2; \
    ls -la "$LIBCLANG_PATH" >&2 2>&1 || true; \
    exit 1

ARG RUST_TOOLCHAIN=nightly-2026-08-20
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --profile minimal \
        --default-toolchain "${RUST_TOOLCHAIN}" \
  && rustc -vV && cargo -V \
  && chmod -R a+rwX /opt/rustup /opt/cargo
