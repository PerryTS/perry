#!/bin/bash
# Optional PTY-backed node:tty parity runner.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURE="$SCRIPT_DIR/fixtures/tty-pty-smoke.ts"
OUTPUT_DIR="$SCRIPT_DIR/output/tty-interactive"
REPORT_DIR="$SCRIPT_DIR/reports"
PERRY_BIN="${PERRY_BIN:-$ROOT_DIR/target/release/perry}"
NODE_BIN="${NODE_BIN:-node}"
COLS="${PERRY_TTY_PTY_COLS:-80}"
ROWS="${PERRY_TTY_PTY_ROWS:-24}"

if [[ "${PERRY_RUN_TTY_INTERACTIVE_TESTS:-0}" != "1" ]]; then
    echo "SKIP tty interactive PTY tests (set PERRY_RUN_TTY_INTERACTIVE_TESTS=1 to run)"
    exit 0
fi

if ! command -v script >/dev/null 2>&1; then
    echo "SKIP tty interactive PTY tests (script(1) not found)"
    exit 0
fi

if ! command -v "$NODE_BIN" >/dev/null 2>&1; then
    echo "FAIL tty interactive PTY tests (NODE_BIN not found: $NODE_BIN)"
    exit 1
fi

mkdir -p "$OUTPUT_DIR/node" "$OUTPUT_DIR/perry" "$REPORT_DIR"

quote_arg() {
    printf "'%s'" "$(printf "%s" "$1" | sed "s/'/'\\\\''/g")"
}

detect_script_style() {
    local probe
    probe=$(mktemp)
    if script -q -e -c "printf __perry_tty_script_ok__" /dev/null >"$probe" 2>&1; then
        if grep -q "__perry_tty_script_ok__" "$probe"; then
            rm -f "$probe"
            echo "util-linux"
            return 0
        fi
    fi
    if script -q /dev/null sh -c "printf __perry_tty_script_ok__" >"$probe" 2>&1; then
        if grep -q "__perry_tty_script_ok__" "$probe"; then
            rm -f "$probe"
            echo "bsd"
            return 0
        fi
    fi
    rm -f "$probe"
    return 1
}

SCRIPT_STYLE="$(detect_script_style || true)"
if [[ -z "$SCRIPT_STYLE" ]]; then
    echo "SKIP tty interactive PTY tests (script(1) could not allocate a usable PTY)"
    exit 0
fi

run_under_pty() {
    local outfile=$1
    local command=$2
    local setup="stty cols $COLS rows $ROWS 2>/dev/null || true"
    if [[ "$SCRIPT_STYLE" == "util-linux" ]]; then
        script -q -e -c "$setup; $command" /dev/null >"$outfile" 2>&1
    else
        script -q /dev/null sh -c "$setup; $command" >"$outfile" 2>&1
    fi
}

normalize_output() {
    perl -0pe 's/\r//g; s/\e\][^\a]*(?:\a|\e\\)//g; s/\e\[[0-?]*[ -\/]*[@-~]//g; s/\e[()][A-Za-z0-9]//g;' |
        sed '/^Script started/d;/^Script done/d'
}

echo "Running optional tty interactive PTY parity (script style: $SCRIPT_STYLE, size: ${COLS}x${ROWS})"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "Building Perry compiler..."
    cargo build --release --quiet -p perry -p perry-runtime -p perry-stdlib
fi

perry_binary="/tmp/perry_tty_pty_smoke"
echo "Compiling fixture with Perry..."
env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE="${PERRY_NO_AUTO_OPTIMIZE:-1}" \
    "$PERRY_BIN" "$FIXTURE" -o "$perry_binary"

node_raw="$OUTPUT_DIR/node/tty-pty-smoke.raw.txt"
perry_raw="$OUTPUT_DIR/perry/tty-pty-smoke.raw.txt"
node_out="$OUTPUT_DIR/node/tty-pty-smoke.txt"
perry_out="$OUTPUT_DIR/perry/tty-pty-smoke.txt"

node_cmd="env FORCE_COLOR=0 NO_COLOR=1 NODE_DISABLE_COLORS=1 TERM=xterm-256color $(quote_arg "$NODE_BIN") --experimental-strip-types $(quote_arg "$FIXTURE")"
perry_cmd="env TERM=xterm-256color $(quote_arg "$perry_binary")"

set +e
run_under_pty "$node_raw" "$node_cmd"
node_status=$?
run_under_pty "$perry_raw" "$perry_cmd"
perry_status=$?
set -e

normalize_output <"$node_raw" >"$node_out"
normalize_output <"$perry_raw" >"$perry_out"

generated_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
report="$REPORT_DIR/tty_interactive_report_$(date -u +"%Y%m%d_%H%M%S").json"
diff_status=0
diff -u "$node_out" "$perry_out" || diff_status=$?

if [[ $node_status -eq 0 && $perry_status -eq 0 && $diff_status -eq 0 ]]; then
    cat >"$report" <<EOF
{
  "generated_at": "$generated_at",
  "suite": "tty-interactive",
  "script_style": "$SCRIPT_STYLE",
  "pty_size": { "columns": $COLS, "rows": $ROWS },
  "node_exit": $node_status,
  "perry_exit": $perry_status,
  "summary": { "parity_pass": 1, "parity_fail": 0, "compile_fail": 0, "skipped": 0 },
  "fixtures": ["test-parity/fixtures/tty-pty-smoke.ts"]
}
EOF
    echo "PASS tty-interactive/tty-pty-smoke"
    echo "Report saved to: $report"
else
    cat >"$report" <<EOF
{
  "generated_at": "$generated_at",
  "suite": "tty-interactive",
  "script_style": "$SCRIPT_STYLE",
  "pty_size": { "columns": $COLS, "rows": $ROWS },
  "node_exit": $node_status,
  "perry_exit": $perry_status,
  "summary": { "parity_pass": 0, "parity_fail": 1, "compile_fail": 0, "skipped": 0 },
  "fixtures": ["test-parity/fixtures/tty-pty-smoke.ts"]
}
EOF
    echo "FAIL tty-interactive/tty-pty-smoke"
    echo "Report saved to: $report"
    exit 1
fi
