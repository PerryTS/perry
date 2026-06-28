#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then
  PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
  echo "Perry binary not found; run cargo build -p perry first" >&2
  exit 1
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat > "$TMPDIR/main.js" <<'JS'
var util = require('util')

function encodeLength(content) {
  content = textEncoder.encode(content)
  return content.byteLength
}

var textEncoder = new util.TextEncoder()

var nestedLen = (function () {
  function encodeLengthNested(content) {
    content = nestedTextEncoder.encode(content)
    return content.byteLength
  }

  var nestedTextEncoder = new util.TextEncoder()
  return encodeLengthNested('world')
})()

console.log('len=' + encodeLength('hello') + ',nested=' + nestedLen)
JS

"$PERRY" compile --no-cache "$TMPDIR/main.js" -o "$TMPDIR/out" >"$TMPDIR/compile.log" 2>&1 || {
  cat "$TMPDIR/compile.log" >&2
  exit 1
}

output="$($TMPDIR/out)"
if [[ "$output" != "len=5,nested=5" ]]; then
  echo "Unexpected output: $output" >&2
  exit 1
fi
