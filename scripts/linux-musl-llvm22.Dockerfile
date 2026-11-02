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
  && /usr/lib/llvm22/bin/llvm-config --version | awk -F. '$1 == 22 && ($2 > 1 || ($2 == 1 && $3+0 >= 5)) { ok=1 } END { exit !ok }' \
     || { echo "alpine LLVM must be 22.x >= 22.1.5 (LLVM #194184) ($(/usr/lib/llvm22/bin/llvm-config --version 2>&1))" >&2; exit 1; }

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

# perry-codegen compiles a C++ adapter against the LLVM headers (DCE layer 0:
# unique sections), and its object links against the harvested libstdc++.a.
# Compile it against that archive's own headers: copy Alpine's include tree
# into a target-only sysroot. `cxx/` names the libstdc++ directories without
# the GCC version, so the build script needs neither the version nor Alpine's
# triple. They must belong to the gcc that located libgcc.a above.
RUN set -eu; \
    sysroot=/opt/perry-musl-sysroot; \
    version="$(gcc -dumpversion)"; \
    triple="$(gcc -dumpmachine)"; \
    if [ ! -s "/usr/include/c++/$version/$triple/bits/c++config.h" ]; then \
      echo "missing Alpine libstdc++ headers for gcc $version ($triple)" >&2; \
      exit 1; \
    fi; \
    mkdir -p "$sysroot/usr" "$sysroot/cxx"; \
    cp -a /usr/include "$sysroot/usr/"; \
    ln -s "../usr/include/c++/$version" "$sysroot/cxx/include"; \
    ln -s "../usr/include/c++/$version/$triple" "$sysroot/cxx/target-include"; \
    echo "musl target C++ headers OK: libstdc++ $version ($triple)"

FROM ${GLIBC_IMAGE}

# The archived slim image has no CA bundle. Debian Release signatures are still
# checked while bootstrapping ca-certificates; only TLS peer validation is
# disabled for this first signed archive fetch. This matches the existing
# glibc-2.31 release image.
RUN printf '%s\n' \
      'deb [check-valid-until=no] https://archive.debian.org/debian bullseye main' \
      'deb https://archive.debian.org/debian-security bullseye-security main' \
      'deb [check-valid-until=no] https://archive.debian.org/debian bullseye-backports main' \
      > /etc/apt/sources.list \
    && apt-get -o Acquire::https::Verify-Peer=false update \
    && DEBIAN_FRONTEND=noninteractive apt-get \
      -o Acquire::https::Verify-Peer=false \
      install -y --no-install-recommends ca-certificates \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      bash build-essential curl file git gnupg libclang-dev musl-tools \
      perl pkg-config python3 \
    # bullseye's CMake 3.18 finds no ar/ranlib behind mbx's compiler shim
    # (CMAKE_AR-NOTFOUND: libz-ng-sys's static archive step); 3.25 does.
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      -t bullseye-backports cmake \
    && curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key \
      -o /etc/apt/trusted.gpg.d/apt.llvm.org.asc \
    && printf '%s\n' \
      'deb https://apt.llvm.org/bullseye/ llvm-toolchain-bullseye-22 main' \
      > /etc/apt/sources.list.d/llvm22.list \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      clang-22 libpolly-22-dev llvm-22-dev libxml2-dev libzstd-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

# perry-runtime's build script compiles its LLVM IR shim (perry_trap.ll) with
# clang. The Alpine LLVM tree below ships no clang, so name a glibc clang 22
# from apt.llvm.org, the release glibc image's source. The GC effects
# analyzer (a host program) links this glibc LLVM 22 too: the Alpine tree is
# musl-built (build_linux_musl.sh points the sidecars step at it).
ENV PERRY_TRAP_CLANG=/usr/lib/llvm-22/bin/clang
ENV PERRY_HOST_LLVM_PREFIX=/usr/lib/llvm-22

# llvm-config itself is an Alpine/musl executable. Copy its tiny runtime to a
# private directory so the glibc host can query it without treating target
# libraries as host libraries. (Linux chooses the embedded musl loader; glibc
# processes continue to use their own loader and multiarch libraries.)
COPY --from=musl-sysroot /usr/lib/llvm22 /usr/lib/llvm22
COPY --from=musl-sysroot /opt/perry-musl /opt/perry-musl
COPY --from=musl-sysroot /opt/perry-musl-sysroot /opt/perry-musl-sysroot
COPY --from=musl-sysroot /lib/ld-musl-*.so.1 /lib/
COPY --from=musl-sysroot /usr/lib/libstdc++.so.6* /opt/perry-musl-runtime/
COPY --from=musl-sysroot /usr/lib/libgcc_s.so.1 /opt/perry-musl-runtime/

ENV LLVM_SYS_221_PREFIX=/usr/lib/llvm22
ENV LIBCLANG_PATH=/opt/perry-host-libclang
ENV BINDGEN_EXTRA_CLANG_ARGS="-isystem /opt/perry-host-libclang/include"

# The musl target's C++ compiler: glibc clang 22 with the target-only sysroot.
# -nostdinc++ and the explicit -isystem order put Alpine's libstdc++ headers
# ahead of musl's C headers (libstdc++'s <cstdlib> include_nexts <stdlib.h>),
# and nothing from the glibc host's /usr/include can be found. The build script
# adds --target per leg.
ENV PERRY_MUSL_CXX=/usr/lib/llvm-22/bin/clang++
ENV PERRY_MUSL_CXXFLAGS="--sysroot=/opt/perry-musl-sysroot -nostdinc++ -isystem /opt/perry-musl-sysroot/cxx/include -isystem /opt/perry-musl-sysroot/cxx/target-include -isystem /opt/perry-musl-sysroot/cxx/include/backward"

# Bindgen must load a GLIBC libclang, never the musl libLLVM tree above. Give it
# a stable path independent of Debian's versioned LLVM directory, then assert
# both halves of the image before a 20-minute Cargo build can start.
RUN set -eu; \
    musl_loader="$(basename "$(find /lib -name 'ld-musl-*.so.1' -print -quit)")"; \
    musl_arch="${musl_loader#ld-musl-}"; \
    musl_arch="${musl_arch%.so.1}"; \
    printf '%s\n' /opt/perry-musl-runtime > "/etc/ld-musl-$musl_arch.path"; \
    # A glob, not find -L: llvm-22-dev's build/ links loop. Only Debian's
    # libclang-dev (LLVM 11) ships an unversioned libclang.so.
    host_libclang="$(ls /usr/lib/llvm-*/lib/libclang.so 2>/dev/null | head -n 1)"; \
    if [ -z "$host_libclang" ]; then \
      echo "Debian libclang-dev installed no libclang.so" >&2; \
      exit 1; \
    fi; \
    host_stdarg="$(find "$(dirname "$host_libclang")/clang" -path '*/include/stdarg.h' -print -quit)"; \
    if [ -z "$host_stdarg" ]; then \
      echo "Debian libclang-dev installed no Clang resource headers" >&2; \
      exit 1; \
    fi; \
    host_clang_include="$(dirname "$host_stdarg")"; \
    mkdir -p "$LIBCLANG_PATH"; \
    ln -s "$host_libclang" "$LIBCLANG_PATH/libclang.so"; \
    ln -s "$host_clang_include" "$LIBCLANG_PATH/include"; \
    ldd "$host_libclang" | grep -q 'libc\.so\.6'; \
    "$LLVM_SYS_221_PREFIX/bin/llvm-config" --version | awk -F. '$1 == 22 && ($2 > 1 || ($2 == 1 && $3+0 >= 5)) { ok=1 } END { exit !ok }'; \
    test -s "$LLVM_SYS_221_PREFIX/lib/libLLVMCore.a"; \
    test -s /opt/perry-musl/lib/libstdc++.a; \
    test -x /usr/bin/musl-gcc; \
    "$PERRY_TRAP_CLANG" --version | grep -q 'clang version 22\.'; \
    # Compile the codegen adapter's one LLVM include exactly as cc-rs will, and
    # require every header it reads from the target sysroot or Alpine's LLVM.
    printf '%s\n' '#include "llvm/Target/TargetMachine.h"' > /tmp/musl-cxx-probe.cpp; \
    "$PERRY_MUSL_CXX" --target="$musl_arch-unknown-linux-musl" $PERRY_MUSL_CXXFLAGS \
      -I "$LLVM_SYS_221_PREFIX/include" -std=c++17 -fsyntax-only -MD -MF /tmp/musl-cxx-probe.d \
      /tmp/musl-cxx-probe.cpp; \
    probe_headers="$(tr -s ' \\' '\n\n' < /tmp/musl-cxx-probe.d | grep '^/' | grep -v '^/tmp/')"; \
    if printf '%s\n' "$probe_headers" | grep -Ev '^(/opt/perry-musl-sysroot/|/usr/lib/llvm22/include/|/usr/lib/llvm-22/lib/clang/22/include/)'; then \
      echo "musl C++ probe read headers outside the target sysroot (above)" >&2; \
      exit 1; \
    fi; \
    printf '%s\n' "$probe_headers" | grep -q '^/opt/perry-musl-sysroot/cxx/target-include/bits/c++config\.h$'; \
    rm -f /tmp/musl-cxx-probe.cpp /tmp/musl-cxx-probe.d; \
    echo "glibc host libclang: $host_libclang"; \
    echo "musl target LLVM: $LLVM_SYS_221_PREFIX"; \
    echo "musl target C++: $PERRY_MUSL_CXX $PERRY_MUSL_CXXFLAGS"
