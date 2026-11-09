#!/bin/bash
# util.styleText colour decision against node: FORCE_COLOR, NO_COLOR,
# NODE_DISABLE_COLORS and TERM, with stdout a pty (script(1)) and a pipe.
# Usage: run_style_text_color_tests.sh  (PERRY_BIN, NODE_BIN override the tools)
# Exit 1 when any case's stdout differs from node's.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE="$SCRIPT_DIR/fixtures/style-text-color-env.ts"
PERRY_BIN="${PERRY_BIN:-$ROOT_DIR/target/release/perry}"
NODE_BIN="${NODE_BIN:-node}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if ! script -q -e -c "printf ok" /dev/null 2>/dev/null | grep -q ok; then
    echo "FAIL style-text color: script(1) (util-linux) cannot allocate a pty"
    exit 1
fi

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE="${PERRY_NO_AUTO_OPTIMIZE:-1}" \
    "$PERRY_BIN" "$FIXTURE" -o "$WORK/style" >"$WORK/compile.log" 2>&1 || {
    echo "FAIL style-text color: perry compile failed (see $WORK/compile.log)"
    trap - EXIT
    exit 1
}

# One case per line: label, then the environment (empty = no colour variables).
CASES=(
    "FORCE_COLOR-unset|TERM=xterm-256color"
    "FORCE_COLOR=''|FORCE_COLOR= TERM=xterm-256color"
    "FORCE_COLOR=0|FORCE_COLOR=0 TERM=xterm-256color"
    "FORCE_COLOR=1|FORCE_COLOR=1 TERM=xterm-256color"
    "FORCE_COLOR=2|FORCE_COLOR=2 TERM=xterm-256color"
    "FORCE_COLOR=3|FORCE_COLOR=3 TERM=xterm-256color"
    "FORCE_COLOR=false|FORCE_COLOR=false TERM=xterm-256color"
    "NO_COLOR=1|NO_COLOR=1 TERM=xterm-256color"
    "NODE_DISABLE_COLORS=1|NODE_DISABLE_COLORS=1 TERM=xterm-256color"
    "TERM=dumb|TERM=dumb"
    "TERM=xterm-256color|TERM=xterm-256color"
    "TERM-unset|"
    "FORCE_COLOR=1+NO_COLOR=1|FORCE_COLOR=1 NO_COLOR=1 TERM=xterm-256color"
    "FORCE_COLOR=''+TERM=dumb|FORCE_COLOR= TERM=dumb"
)

# A fixed, colour-free base environment: the case adds its own variables.
BASE="env -i PATH=/usr/bin:/bin HOME=$WORK"

run_case() { # mode label envv cmd -> stdout file
    local mode=$1 envv=$2 cmd=$3 out=$4
    if [[ $mode == tty ]]; then
        script -q -e -c "$BASE $envv $cmd 2>/dev/null" /dev/null </dev/null | tr -d '\r' >"$out" || true
    else
        bash -c "$BASE $envv $cmd 2>/dev/null | cat" </dev/null >"$out" || true
    fi
}

fail=0
printf "%-28s %-5s %-6s %s\n" case mode result "node red line"
for c in "${CASES[@]}"; do
    label=${c%%|*}; envv=${c#*|}
    for mode in tty pipe; do
        run_case $mode "$envv" "$NODE_BIN --experimental-strip-types $FIXTURE" "$WORK/node.out"
        run_case $mode "$envv" "$WORK/style" "$WORK/perry.out"
        if [[ -s $WORK/node.out ]] && cmp -s "$WORK/node.out" "$WORK/perry.out"; then r=PASS; else r=FAIL; fail=1; fi
        printf "%-28s %-5s %-6s %s\n" "$label" $mode $r "$(grep '^red ' "$WORK/node.out" || echo '<no output>')"
        if [[ $r == FAIL ]]; then diff "$WORK/node.out" "$WORK/perry.out" | sed 's/^/    /' || true; fi
    done
done
if [[ $fail -eq 0 ]]; then echo "PASS style-text color"; else echo "FAIL style-text color"; exit 1; fi
