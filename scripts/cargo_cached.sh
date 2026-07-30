#!/usr/bin/env bash
set -euo pipefail

command -v cargo >/dev/null 2>&1 || {
  echo 'cargo_cached: cargo not found in PATH' >&2
  exit 127
}
command -v sccache >/dev/null 2>&1 || {
  echo 'cargo_cached: sccache not found in PATH' >&2
  exit 127
}

export RUSTC_WRAPPER=sccache
export CARGO_INCREMENTAL=0
if [ -z "${SCCACHE_DIR:-}" ] && [ -z "${XDG_CACHE_HOME:-}" ] && [ -z "${HOME:-}" ]; then
  echo 'cargo_cached: HOME is not set and SCCACHE_DIR/XDG_CACHE_HOME are unset' >&2
  exit 1
fi
export SCCACHE_DIR="${SCCACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/perry/sccache}"
export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-12G}"

mkdir -p -- "$SCCACHE_DIR"
exec cargo "$@"
