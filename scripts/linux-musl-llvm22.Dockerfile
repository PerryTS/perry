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
      zlib-static zstd-static libxml2-static ncurses-static \
      musl-dev pkgconf perl python3 \
  # Alpine installs LLVM 22 under /usr/lib/llvm22 and does NOT put its
  # llvm-config on PATH (the bare name is unversioned and absent), so the
  # check has to name the prefix explicitly.
  && /usr/lib/llvm22/bin/llvm-config --version | grep -q '^22\.' \
     || { echo "alpine llvm is not 22.x ($(/usr/lib/llvm22/bin/llvm-config --version 2>&1))" >&2; exit 1; }

# llvm-sys links LLVM statically, so every library `llvm-config --system-libs`
# names must exist as a `.a` here. Alpine's `-dev` packages carry only the
# shared object, so a missing `-static` package surfaces ~20 minutes into the
# cargo build as `could not find native static library 'z'` rather than at
# image-build time. Assert it up front instead: resolve each `-lNAME` to a
# `libNAME.a`, skipping the names musl folds into libc.a (which ships stubs,
# not standalone archives).
RUN set -eu;     libdirs="$(/usr/lib/llvm22/bin/llvm-config --libdir) /usr/lib /usr/lib/gcc";     missing="";     for flag in $(/usr/lib/llvm22/bin/llvm-config --system-libs); do       name="${flag#-l}";       [ "$name" != "$flag" ] || continue;       case " c m rt dl pthread util xnet " in *" $name "*) continue ;; esac;       found="";       for d in $libdirs; do         [ -e "$d/lib$name.a" ] && { found=1; break; };       done;       [ -n "$found" ] || missing="$missing $name";     done;     if [ -n "$missing" ]; then       echo "missing static system libs for llvm-sys:$missing" >&2;       echo "(llvm-config --system-libs: $(/usr/lib/llvm22/bin/llvm-config --system-libs))" >&2;       exit 1;     fi;     echo "static system libs OK: $(/usr/lib/llvm22/bin/llvm-config --system-libs)"

ENV LLVM_SYS_221_PREFIX=/usr/lib/llvm22
ENV RUSTUP_HOME=/opt/rustup
ENV CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:/usr/lib/llvm22/bin:$PATH

ARG RUST_TOOLCHAIN=nightly-2026-08-20
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --profile minimal \
        --default-toolchain "${RUST_TOOLCHAIN}" \
  && rustc -vV && cargo -V \
  && chmod -R a+rwX /opt/rustup /opt/cargo
