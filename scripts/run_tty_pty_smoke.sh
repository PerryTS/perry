#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE="$ROOT/test-parity/fixtures/tty-pty-smoke.ts"
OUT_DIR="$ROOT/target/tmp/tty-pty-smoke"
PERRY_BIN="$ROOT/target/release/perry"
PERRY_OUT="$OUT_DIR/perry-tty-pty-smoke"
BUILD_LOG="$OUT_DIR/build.log"
COMPILE_LOG="$OUT_DIR/compile.log"
NODE_BIN="${NODE_BIN:-/home/github-runner/actions-runner/externals/node24/bin/node}"

if [[ ! -x "$NODE_BIN" ]]; then
  NODE_BIN="$(command -v node)"
fi

mkdir -p "$OUT_DIR"

cargo build --release --quiet -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static >"$BUILD_LOG" 2>&1 || {
  cat "$BUILD_LOG"
  exit 1
}

PERRY_ALLOW_UNIMPLEMENTED=1 "$PERRY_BIN" "$FIXTURE" -o "$PERRY_OUT" >"$COMPILE_LOG" 2>&1 || {
  cat "$COMPILE_LOG"
  exit 1
}

run_under_pty() {
  local output_file="$1"
  local command="$2"
  script -q -e -c "stty rows 24 cols 80 < /dev/tty; (sleep 0.45; stty rows 31 cols 100 < /dev/tty; kill -WINCH \$\$ 2>/dev/null || true) & exec $command" /dev/null >"$output_file" 2>&1
}

run_under_pty "$OUT_DIR/node.raw" "'$NODE_BIN' --experimental-strip-types '$FIXTURE'"
run_under_pty "$OUT_DIR/perry.raw" "'$PERRY_OUT'"

normalize() {
  python3 - "$1" <<'PY'
import re
import sys
path = sys.argv[1]
text = open(path, "rb").read().decode("utf-8", "replace")
text = text.replace("\r", "")
text = re.sub(r"\x1b\[[0-9;?]*[ -/]*[@-~]", "", text)
text = "\n".join(line.rstrip() for line in text.splitlines() if line.strip())
print(text)
PY
}

normalize "$OUT_DIR/node.raw" >"$OUT_DIR/node.txt"
normalize "$OUT_DIR/perry.raw" >"$OUT_DIR/perry.txt"

if ! diff -u "$OUT_DIR/node.txt" "$OUT_DIR/perry.txt"; then
  echo "TTY PTY smoke mismatch; raw output kept in $OUT_DIR" >&2
  exit 1
fi

cat "$OUT_DIR/perry.txt"
