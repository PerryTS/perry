#!/usr/bin/env bash
# Build the Linux GNU release payload inside a glibc 2.31 container.
#
# GTK4 is deliberately absent here. Its WebKitGTK/libshumate dependencies need
# the Ubuntu 24.04 host used by release-packages.yml; that job adds the UI
# archive after this script has produced the compiler and non-UI archives.

set -euo pipefail

target="${1:?usage: build_linux_glibc_2_31.sh <rust-target>}"
target_dir="${CARGO_TARGET_DIR:-target}"
abort_target_dir="${PERRY_ABORT_TARGET_DIR:-target-abort}"

case "$target" in
  x86_64-unknown-linux-gnu)
    expected_machine=x86_64
    ;;
  aarch64-unknown-linux-gnu)
    expected_machine=aarch64
    ;;
  *)
    echo "unsupported old-glibc release target: $target" >&2
    exit 2
    ;;
esac

machine=$(uname -m)
if [ "$machine" != "$expected_machine" ]; then
  echo "container architecture is $machine; $target needs $expected_machine" >&2
  exit 1
fi

glibc_version=$(getconf GNU_LIBC_VERSION | awk '{print $2}')
if [ "$glibc_version" != "2.31" ]; then
  echo "expected the glibc 2.31 sysroot, found glibc $glibc_version" >&2
  exit 1
fi

: "${LLVM_SYS_221_PREFIX:=/usr/lib/llvm-22}"
export LLVM_SYS_221_PREFIX
"$LLVM_SYS_221_PREFIX/bin/llvm-config" --version | grep -q '^22\.' || {
  echo "old-sysroot image does not provide LLVM 22 under $LLVM_SYS_221_PREFIX" >&2
  exit 1
}

rustup target add "$target"

cargo build --profile dist --target "$target" -p perry
cargo build --profile dist --target "$target" -p perry-runtime -p perry-runtime-static
cargo build --profile dist --target "$target" -p perry-stdlib -p perry-stdlib-static

# Ship the panic=abort runtime variant from the same old sysroot.
CARGO_TARGET_DIR="$abort_target_dir" CARGO_PROFILE_DIST_PANIC=abort \
  cargo build --profile dist --target "$target" -p perry-runtime -p perry-runtime-static
cp "$abort_target_dir/$target/dist/libperry_runtime.a" \
   "$target_dir/$target/dist/libperry_runtime_abort.a"

# Match the ordinary release leg's best-effort extension-library build. #5716:
# enumerate the explicit governance inventory rather than every matching
# directory. Keep perry and both wrappers in each invocation so Cargo resolves
# one feature union and the final linker can deduplicate shared dependencies.
governed_ext_packages=$(./scripts/release_ext_packages.sh)
while IFS= read -r package; do
  echo "::group::build $package"
  cargo build --profile dist --target "$target" \
    -p perry -p perry-runtime-static -p perry-stdlib-static -p "$package" \
    || echo "  (skipped $package -- failed to build on the glibc 2.31 sysroot)"
  echo "::endgroup::"
done <<< "$governed_ext_packages"

compiler="$target_dir/$target/dist/perry"
max_glibc=$(
  readelf --version-info "$compiler" \
    | grep -oE 'GLIBC_[0-9]+(\.[0-9]+)+' \
    | sed 's/^GLIBC_//' \
    | sort -Vu \
    | tail -1
)
if [ -z "$max_glibc" ] || [ "$(printf '%s\n' "$max_glibc" "2.31" | sort -V | tail -1)" != "2.31" ]; then
  echo "compiler requires GLIBC_${max_glibc:-unknown}; expected no newer than GLIBC_2.31" >&2
  exit 1
fi

# Exercise both the compiler (which statically links LLVM 22) and the shipped
# runtime/stdlib archives before the noble-built GTK4 archive is added.
smoke_dir=$(mktemp -d)
trap 'rm -rf "$smoke_dir"' EXIT
cat > "$smoke_dir/hello.ts" <<'TS'
const values = [1, 2, 3].map((value) => value * 2);
console.log(`old-glibc ${values.join(",")}`);
TS
"$compiler" --version
"$compiler" "$smoke_dir/hello.ts" -o "$smoke_dir/hello"
test "$("$smoke_dir/hello")" = "old-glibc 2,4,6"

echo "Linux GNU release payload built and exercised on glibc $glibc_version (max symbol GLIBC_$max_glibc)"
