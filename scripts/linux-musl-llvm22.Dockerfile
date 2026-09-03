# Cross-build image for the Linux musl release legs.
#
# Perry links libLLVM in-process, so the target needs a musl-built LLVM. Alpine
# is the only packaged source of LLVM 22 for musl, but Cargo build scripts must
# NOT run there: `rusqlite/session` makes libsqlite3-sys run bindgen, and a
# musl-host build script is crt-static and cannot dlopen libclang (#9382).
#
# Keep those two concerns separate. The first stage harvests Alpine's
# architecture-matched LLVM and C++ archives. The final stage is glibc, so
# rustup installs a GNU-host toolchain and bindgen can dlopen the host libclang;
# rustc still targets `*-unknown-linux-musl` and links the harvested archives.
ARG ALPINE_IMAGE=alpine:edge
ARG GLIBC_IMAGE=debian:bullseye-slim@sha256:f313b4bd62667092a59b3a664d7d3ab8b5e65f41675f48e81455a15dc5abe792

FROM ${ALPINE_IMAGE} AS musl-sysroot

# Alpine 22.1.8 is the exact LLVM series llvm-sys 221 requires. `build-base`
# supplies the matching musl libstdc++/libgcc archives; the remaining `-static`
# packages satisfy llvm-config's system-library list at the final target link.
RUN apk add --no-cache \
      build-base \
      llvm22 llvm22-dev llvm22-static llvm22-libs \
      libffi-dev zlib-dev zstd-dev xz-dev \
      zlib-static zstd-static libxml2-static ncurses-static \
  && /usr/lib/llvm22/bin/llvm-config --version | grep -q '^22\.' \
     || { echo "alpine llvm is not 22.x ($(/usr/lib/llvm22/bin/llvm-config --version 2>&1))" >&2; exit 1; }

# Keep target libraries out of the glibc host's normal /usr/lib search path.
# Only target-specific rustflags expose this directory to rustc, preventing a
# host build script from accidentally linking a musl archive.
RUN set -eu; \
    mkdir -p /opt/perry-musl/lib; \
    required="ffi rt dl m z zstd xml2 stdc++"; \
    for name in $required; do \
      src="/usr/lib/lib$name.a"; \
      if [ ! -e "$src" ]; then \
        echo "missing Alpine target archive: $src" >&2; \
        exit 1; \
      fi; \
      cp -L "$src" /opt/perry-musl/lib/; \
    done; \
    for name in gcc gcc_eh atomic; do \
      src="$(gcc -print-file-name="lib$name.a")"; \
      if [ "$src" = "lib$name.a" ] || [ ! -e "$src" ]; then \
        echo "missing Alpine target archive: lib$name.a" >&2; \
        exit 1; \
      fi; \
      cp -L "$src" /opt/perry-musl/lib/; \
    done; \
    for name in ffi rt dl m z zstd xml2; do \
      printf 'INPUT ( lib%s.a )\n' "$name" > "/opt/perry-musl/lib/lib$name.so"; \
    done; \
    # llvm-sys chooses the system-library link kind when its glibc-hosted \
    # build script is compiled, so it emits `-lfoo` instead of a static-kind \
    # directive for this musl target. Target-only .so linker scripts map those \
    # names back to the harvested archives; the build script's readelf gate \
    # verifies that no dynamic dependency escapes into the release artifact. \
    printf '%s\n' 'GROUP ( libstdc++.a libgcc_eh.a libgcc.a libatomic.a )' \
      > /opt/perry-musl/lib/libstdc++.so; \
    reported="$(/usr/lib/llvm22/bin/llvm-config --system-libs --link-static)"; \
    for required_flag in -lz -lzstd -lxml2; do \
      case " $reported " in \
        *" $required_flag "*) ;; \
        *) echo "llvm-config omitted required system lib $required_flag: $reported" >&2; exit 1 ;; \
      esac; \
    done; \
    echo "musl target libraries OK: $reported"

FROM ${GLIBC_IMAGE}

# The archived slim image has no CA bundle. Debian Release signatures are still
# checked while bootstrapping ca-certificates; only TLS peer validation is
# disabled for this first signed archive fetch. This matches the existing
# glibc-2.31 release image.
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
      bash build-essential cmake curl file git libclang-dev musl-tools \
      perl pkg-config python3 \
    && rm -rf /var/lib/apt/lists/*

# llvm-config itself is an Alpine/musl executable. Copy its tiny runtime to a
# private directory so the glibc host can query it without treating target
# libraries as host libraries. (Linux chooses the embedded musl loader; glibc
# processes continue to use their own loader and multiarch libraries.)
COPY --from=musl-sysroot /usr/lib/llvm22 /usr/lib/llvm22
COPY --from=musl-sysroot /opt/perry-musl /opt/perry-musl
COPY --from=musl-sysroot /lib/ld-musl-*.so.1 /lib/
COPY --from=musl-sysroot /usr/lib/libstdc++.so.6* /opt/perry-musl-runtime/
COPY --from=musl-sysroot /usr/lib/libgcc_s.so.1 /opt/perry-musl-runtime/

ENV LLVM_SYS_221_PREFIX=/usr/lib/llvm22
ENV LIBCLANG_PATH=/opt/perry-host-libclang
ENV BINDGEN_EXTRA_CLANG_ARGS="-isystem /opt/perry-host-libclang/include"

# Bindgen must load a GLIBC libclang, never the musl libLLVM tree above. Give it
# a stable path independent of Debian's versioned LLVM directory, then assert
# both halves of the image before a 20-minute Cargo build can start.
RUN set -eu; \
    musl_loader="$(basename "$(find /lib -name 'ld-musl-*.so.1' -print -quit)")"; \
    musl_arch="${musl_loader#ld-musl-}"; \
    musl_arch="${musl_arch%.so.1}"; \
    printf '%s\n' /opt/perry-musl-runtime > "/etc/ld-musl-$musl_arch.path"; \
    host_libclang="$(find -L /usr/lib -path '*/llvm-*/lib/libclang.so' -print -quit)"; \
    if [ -z "$host_libclang" ]; then \
      echo "Debian libclang-dev installed no libclang.so" >&2; \
      exit 1; \
    fi; \
    host_stdarg="$(find /usr/lib/llvm-* -path '*/lib/clang/*/include/stdarg.h' -print -quit)"; \
    if [ -z "$host_stdarg" ]; then \
      echo "Debian libclang-dev installed no Clang resource headers" >&2; \
      exit 1; \
    fi; \
    host_clang_include="$(dirname "$host_stdarg")"; \
    mkdir -p "$LIBCLANG_PATH"; \
    ln -s "$host_libclang" "$LIBCLANG_PATH/libclang.so"; \
    ln -s "$host_clang_include" "$LIBCLANG_PATH/include"; \
    ldd "$host_libclang" | grep -q 'libc\.so\.6'; \
    "$LLVM_SYS_221_PREFIX/bin/llvm-config" --version | grep -q '^22\.'; \
    test -s "$LLVM_SYS_221_PREFIX/lib/libLLVMCore.a"; \
    test -s /opt/perry-musl/lib/libstdc++.a; \
    test -x /usr/bin/musl-gcc; \
    echo "glibc host libclang: $host_libclang"; \
    echo "musl target LLVM: $LLVM_SYS_221_PREFIX"
