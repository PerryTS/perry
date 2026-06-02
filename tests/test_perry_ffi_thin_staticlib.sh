#!/bin/bash
# Regression: native wrapper staticlibs that depend on perry-ffi must not
# embed perry-runtime object code. The final Perry link already provides the
# runtime archive; wrapper archives should carry unresolved references to the
# runtime ABI symbols instead of duplicate definitions.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."

if ! command -v cargo >/dev/null 2>&1; then
  echo "SKIP: cargo not available"
  exit 0
fi

if ! command -v ar >/dev/null 2>&1; then
  echo "SKIP: ar not available"
  exit 0
fi

NM=""
if command -v llvm-nm >/dev/null 2>&1; then
  NM="$(command -v llvm-nm)"
elif command -v xcrun >/dev/null 2>&1 && xcrun --find llvm-nm >/dev/null 2>&1; then
  NM="$(xcrun --find llvm-nm)"
elif command -v nm >/dev/null 2>&1; then
  NM="$(command -v nm)"
else
  echo "SKIP: nm/llvm-nm not available"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

mkdir -p "$TMPDIR/native/src"

cat > "$TMPDIR/native/Cargo.toml" << EOF
[package]
name = "perry_issue1027_native"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib", "rlib"]

[dependencies]
perry-ffi = { path = "$REPO_ROOT/crates/perry-ffi" }
EOF

cat > "$TMPDIR/native/src/lib.rs" << 'EOF'
use perry_ffi::{alloc_string, gc_register_root_scanner, read_string, JsString, StringHeader};

fn scan_issue1027_roots(mark: &mut dyn FnMut(f64)) {
    mark(f64::from_bits(0x7ffc_0000_0000_0001));
}

#[no_mangle]
pub extern "C" fn js_issue1027_register_roots() {
    gc_register_root_scanner(scan_issue1027_roots);
}

#[no_mangle]
pub extern "C" fn js_issue1027_echo(input: *mut StringHeader) -> *mut StringHeader {
    let input = unsafe { JsString::from_raw(input) };
    let value = read_string(input).unwrap_or("missing");
    alloc_string(value).as_raw()
}
EOF

cargo build --quiet --release --manifest-path "$TMPDIR/native/Cargo.toml"

ARCHIVE="$TMPDIR/native/target/release/libperry_issue1027_native.a"
MEMBERS="$TMPDIR/archive-members.txt"
SYMBOLS="$TMPDIR/symbols.txt"
NM_ERRORS="$TMPDIR/nm-errors.txt"

ar t "$ARCHIVE" > "$MEMBERS"

if grep -q 'perry_runtime-' "$MEMBERS"; then
  echo "FAIL: native wrapper archive embeds perry-runtime objects"
  grep 'perry_runtime-' "$MEMBERS" | head -20
  exit 1
fi

"$NM" -g "$ARCHIVE" > "$SYMBOLS" 2>"$NM_ERRORS" || true
if [ ! -s "$SYMBOLS" ]; then
  "$NM" "$ARCHIVE" > "$SYMBOLS" 2>>"$NM_ERRORS" || true
fi

for symbol in js_string_from_bytes perry_ffi_gc_register_root_scanner; do
  if grep -Eq "[[:space:]]T[[:space:]]_?${symbol}$" "$SYMBOLS"; then
    echo "FAIL: wrapper archive defines runtime symbol $symbol"
    grep -E "_?${symbol}$" "$SYMBOLS"
    exit 1
  fi
  if ! grep -Eq "[[:space:]]U[[:space:]]_?${symbol}$" "$SYMBOLS"; then
    echo "FAIL: wrapper archive does not leave $symbol unresolved"
    grep -E "_?${symbol}$" "$SYMBOLS" || true
    exit 1
  fi
done

echo "PASS"
