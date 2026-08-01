#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WRAPPER="$ROOT/scripts/cargo_cached.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

FAKE_BIN="$TMP_DIR/bin"
mkdir -p "$FAKE_BIN" "$TMP_DIR/home"

cat >"$FAKE_BIN/cargo" <<'EOF'
#!/usr/bin/env bash
{
  printf 'RUSTC_WRAPPER=%s\n' "$RUSTC_WRAPPER"
  printf 'CARGO_INCREMENTAL=%s\n' "$CARGO_INCREMENTAL"
  printf 'SCCACHE_DIR=%s\n' "$SCCACHE_DIR"
  printf 'SCCACHE_CACHE_SIZE=%s\n' "$SCCACHE_CACHE_SIZE"
  printf 'argc=%s\n' "$#"
  i=0
  for arg in "$@"; do
    printf 'arg%s=%s\n' "$i" "$arg"
    i=$((i + 1))
  done
} >"$CAPTURE"
EOF

cat >"$FAKE_BIN/sccache" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$FAKE_BIN/cargo" "$FAKE_BIN/sccache"

assert_line() {
  grep -Fx -- "$2" "$1" >/dev/null || {
    printf 'missing line %s in %s\n' "$2" "$1" >&2
    exit 1
  }
}

DEFAULT_CAPTURE="$TMP_DIR/default.txt"
(
  unset XDG_CACHE_HOME SCCACHE_DIR SCCACHE_CACHE_SIZE
  export HOME="$TMP_DIR/home" PATH="$FAKE_BIN:/usr/bin:/bin" CAPTURE="$DEFAULT_CAPTURE"
  "$WRAPPER" build --package "perry cli"
)

DEFAULT_CACHE="$TMP_DIR/home/.cache/perry/sccache"
assert_line "$DEFAULT_CAPTURE" 'RUSTC_WRAPPER=sccache'
assert_line "$DEFAULT_CAPTURE" 'CARGO_INCREMENTAL=0'
assert_line "$DEFAULT_CAPTURE" "SCCACHE_DIR=$DEFAULT_CACHE"
assert_line "$DEFAULT_CAPTURE" 'SCCACHE_CACHE_SIZE=12G'
assert_line "$DEFAULT_CAPTURE" 'argc=3'
assert_line "$DEFAULT_CAPTURE" 'arg0=build'
assert_line "$DEFAULT_CAPTURE" 'arg1=--package'
assert_line "$DEFAULT_CAPTURE" 'arg2=perry cli'
test -d "$DEFAULT_CACHE"
case "$DEFAULT_CACHE" in
  "$ROOT"/*) echo "default cache must be outside the repository" >&2; exit 1 ;;
esac

XDG_CAPTURE="$TMP_DIR/xdg.txt"
XDG_CACHE="$TMP_DIR/xdg/perry/sccache"
HOME="$TMP_DIR/home" XDG_CACHE_HOME="$TMP_DIR/xdg" \
  PATH="$FAKE_BIN:/usr/bin:/bin" CAPTURE="$XDG_CAPTURE" \
  "$WRAPPER" check
assert_line "$XDG_CAPTURE" "SCCACHE_DIR=$XDG_CACHE"
test -d "$XDG_CACHE"

OVERRIDE_CAPTURE="$TMP_DIR/override.txt"
OVERRIDE_CACHE="$TMP_DIR/custom-cache"
HOME="$TMP_DIR/home" PATH="$FAKE_BIN:/usr/bin:/bin" CAPTURE="$OVERRIDE_CAPTURE" \
  SCCACHE_DIR="$OVERRIDE_CACHE" SCCACHE_CACHE_SIZE=3G \
  "$WRAPPER" check -p perry
assert_line "$OVERRIDE_CAPTURE" "SCCACHE_DIR=$OVERRIDE_CACHE"
assert_line "$OVERRIDE_CAPTURE" 'SCCACHE_CACHE_SIZE=3G'
test -d "$OVERRIDE_CACHE"

CARGO_ONLY="$TMP_DIR/cargo-only"
mkdir -p "$CARGO_ONLY"
cp "$FAKE_BIN/cargo" "$CARGO_ONLY/cargo"
ln -s "$(command -v bash)" "$CARGO_ONLY/bash"
set +e
MISSING_OUTPUT="$(PATH="$CARGO_ONLY" "$WRAPPER" check 2>&1)"
MISSING_STATUS=$?
set -e
test "$MISSING_STATUS" -eq 127
test "$MISSING_OUTPUT" = 'cargo_cached: sccache not found in PATH'

CACHE_ONLY="$TMP_DIR/cache-only"
mkdir -p "$CACHE_ONLY"
cp "$FAKE_BIN/sccache" "$CACHE_ONLY/sccache"
ln -s "$(command -v bash)" "$CACHE_ONLY/bash"
set +e
MISSING_OUTPUT="$(PATH="$CACHE_ONLY" "$WRAPPER" check 2>&1)"
MISSING_STATUS=$?
set -e
test "$MISSING_STATUS" -eq 127
test "$MISSING_OUTPUT" = 'cargo_cached: cargo not found in PATH'

set +e
MISSING_OUTPUT="$(
  unset HOME XDG_CACHE_HOME SCCACHE_DIR
  PATH="$FAKE_BIN:/usr/bin:/bin" "$WRAPPER" check 2>&1
)"
MISSING_STATUS=$?
set -e
test "$MISSING_STATUS" -eq 1
test "$MISSING_OUTPUT" = 'cargo_cached: HOME is not set and SCCACHE_DIR/XDG_CACHE_HOME are unset'

echo 'cargo cached wrapper: ok'
