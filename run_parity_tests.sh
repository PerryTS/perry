#!/bin/bash
# Perry Parity Test Runner
# Compares output between Node.js and Perry native compilation

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEST_DIR="$SCRIPT_DIR/test-files"
NODE_SUITE_DIR="$SCRIPT_DIR/test-parity/node-suite"
OUTPUT_DIR="$SCRIPT_DIR/test-parity/output"
REPORT_DIR="$SCRIPT_DIR/test-parity/reports"
# Per-run scratch dir for compiled test binaries (2026-07-02 audit): the old
# fixed /tmp/perry_parity_<test-id> paths meant two concurrent suite runs
# (two agents / two worktrees on one machine) executed EACH OTHER'S compiler
# output — cross-contaminated pass/fail attributed to the wrong build.
PARITY_TMP="$(mktemp -d "${TMPDIR:-/tmp}/perry-parity.XXXXXX")"
trap 'rm -rf "$PARITY_TMP"' EXIT

# LLVM is the only backend post-Phase K hard cutover. The --llvm /
# --cranelift flags and PERRY_BACKEND env var are kept as no-ops for
# backward compat with existing scripts.
BACKEND_FLAG=""
BACKEND_LABEL="LLVM"

# Optional substring filter — only test files whose basename contains
# this string get executed. Useful for iterating on a subset:
#   ./run_parity_tests.sh --filter parity_url
#   ./run_parity_tests.sh --filter parity_     # all parity-inventory tests
TEST_FILTER=""
# Optional suite selector. The historical default (`all`) keeps running the
# top-level test-files/*.ts corpus. The granular `node-suite` selector runs
# curated Node-compatibility cases under test-parity/node-suite/<module>/...
# without requiring test_parity_* names.
TEST_SUITE="all"
MODULE_FILTER=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter) TEST_FILTER="$2"; shift 2 ;;
        --filter=*) TEST_FILTER="${1#--filter=}"; shift ;;
        --suite) TEST_SUITE="$2"; shift 2 ;;
        --suite=*) TEST_SUITE="${1#--suite=}"; shift ;;
        --module) MODULE_FILTER="$2"; shift 2 ;;
        --module=*) MODULE_FILTER="${1#--module=}"; shift ;;
        *) shift ;;
    esac
done

case "$TEST_SUITE" in
    all|parity|smoke|node-suite) ;;
    *)
        echo -e "\033[0;31mUnknown suite: $TEST_SUITE\033[0m"
        echo "Known suites: all, parity, smoke, node-suite"
        exit 1
        ;;
esac

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Find timeout command (GNU coreutils on Linux, gtimeout on macOS via Homebrew)
if command -v timeout &> /dev/null; then
    TIMEOUT_CMD="timeout"
elif command -v gtimeout &> /dev/null; then
    TIMEOUT_CMD="gtimeout"
else
    # No timeout available - run without timeout
    TIMEOUT_CMD=""
fi

# Function to run with optional timeout
run_with_timeout() {
    local seconds=$1
    shift
    if [[ -n "$TIMEOUT_CMD" ]]; then
        $TIMEOUT_CMD "$seconds" "$@"
    else
        "$@"
    fi
}

wait_for_tcp_port() {
    local host=$1
    local port=$2
    local attempts=$3
    local delay=${4:-0.1}
    python3 - "$host" "$port" "$attempts" "$delay" <<'PY'
import socket
import sys
import time

host = sys.argv[1]
port = int(sys.argv[2])
attempts = int(sys.argv[3])
delay = float(sys.argv[4])

for _ in range(attempts):
    try:
        with socket.create_connection((host, port), timeout=0.2):
            sys.exit(0)
    except OSError:
        time.sleep(delay)

sys.exit(1)
PY
}

# ── TLS-upgrade companion server (issue #275) ──────────────────────────────
# Spawned once per test_net_upgrade_tls* test; killed immediately after.
# Uses a self-signed cert; test calls upgradeToTLS(host, verify=0) so cert
# validation is intentionally disabled on the client side.

TLS_UPGRADE_SERVER_PID=""

start_tls_upgrade_server() {
    local server_script="$SCRIPT_DIR/test-files/test_net_upgrade_tls_server.py"
    if ! command -v python3 &>/dev/null; then
        echo -e "${YELLOW}WARN${NC}  python3 not found — test_net_upgrade_tls will fail parity" >&2
        return 1
    fi
    if [[ ! -f "$server_script" ]]; then
        echo -e "${YELLOW}WARN${NC}  $server_script not found — test_net_upgrade_tls will fail parity" >&2
        return 1
    fi
    python3 "$server_script" --port 17892 &
    TLS_UPGRADE_SERVER_PID=$!
    # Wait up to 3 s for the port to open.
    wait_for_tcp_port 127.0.0.1 17892 30 0.1 && return 0
    echo -e "${YELLOW}WARN${NC}  TLS-upgrade server did not come up in time (pid $TLS_UPGRADE_SERVER_PID)" >&2
    return 1
}

stop_tls_upgrade_server() {
    if [[ -n "$TLS_UPGRADE_SERVER_PID" ]]; then
        kill "$TLS_UPGRADE_SERVER_PID" 2>/dev/null || true
        wait "$TLS_UPGRADE_SERVER_PID" 2>/dev/null || true
        TLS_UPGRADE_SERVER_PID=""
    fi
}

# ── Perry-specific expected-output tests ────────────────────────────────────
# Some tests use Perry APIs that don't map 1:1 to Node.js (e.g. Perry's
# net.createConnection(host, port) vs Node.js's (port, host)).  For these,
# instead of comparing to Node.js, we compare Perry's output against a
# stored expected file in test-parity/expected/<test_name>.txt.
# Node.js is still run; if it exits non-zero we record NODE_FAIL and skip;
# if it exits 0 but with a different output we fall through to the expected-
# file comparison (not a parity fail — the incompatibility is intentional).
EXPECTED_DIR="$SCRIPT_DIR/test-parity/expected"
EXPECTED_EXIT_DIR="$SCRIPT_DIR/test-parity/expected-exit"

has_expected_output() {
    [[ -f "$EXPECTED_DIR/${1}.txt" ]]
}

expected_exit_code() {
    local test_name=$1
    if [[ -f "$EXPECTED_EXIT_DIR/${test_name}.txt" ]]; then
        tr -d '[:space:]' < "$EXPECTED_EXIT_DIR/${test_name}.txt"
    else
        printf "0"
    fi
}

# ── Counters ────────────────────────────────────────────────────────────────
PARITY_PASS=0
PARITY_FAIL=0
COMPILE_FAIL=0
NODE_FAIL=0
SKIPPED=0

# Arrays for tracking
declare -a PARITY_FAILURES=()
declare -a COMPILE_FAILURES=()

# Create output directories
mkdir -p "$OUTPUT_DIR/node" "$OUTPUT_DIR/perry" "$REPORT_DIR"

# Tests to skip (random-dependent tests, etc.)
SKIP_TESTS=(
    # test_async / _async2 / _async3 / _async4 / _async5 / _async_chain were
    # un-skipped in v0.5.509 — fixed by the v0.5.508 ABI fix on
    # js_object_set_field (the synthesized async-iter object's closure
    # fields had been storing 0 due to the same bug as #448 / #451).
    "test_timer"
    # Tests with inherently non-deterministic output
    "test_date"      # timestamps differ
    "test_math"      # Math.random() differs
    "test_require"   # crypto.randomUUID() differs
    # Tests that use TypeScript features not supported by Node.js --experimental-strip-types
    "test_enum"             # TS enums need transformation
    "test_decorators"       # TS decorators need transformation
    # Tests that need specific Node.js imports
    "test_crypto"           # crypto.randomBytes needs import
    "test_fs"               # fs module needs import
    "test_path"             # path module needs import
    "test_integration_app"  # uses fs module
    # Network tests — test_net_min and test_net_socket are handled by the
    # plain TCP echo-server lifecycle (start_echo_server / stop_echo_server,
    # added in #286). test_net_upgrade_tls is handled by the TLS-upgrade
    # companion server spawned inline below (#288). test_tls_connect needs
    # outbound HTTPS to example.com:443 — skip unconditionally.
    "test_tls_connect"
    # Timing benchmarks — print Date.now() deltas which differ
    # run-to-run. Both perry and node produce correct output;
    # the parity diff is just measurement noise.
    "test_issue58_object_string"
    "test_issue63_arr"
    "test_issue63_escape"
    # `test_issue63_asm` prints sink.length (deterministic), keep it.
)

# Function to check if test should be skipped
should_skip() {
    local test_name=$1
    for skip in "${SKIP_TESTS[@]}"; do
        if [[ "$test_name" == "$skip" ]]; then
            return 0
        fi
    done
    return 1
}

# Issue #796 — per-test output cap. Pathological output
# (test_parity_timers_promises emitted 5.7M lines pre-fix, root cause
# #712) DOSed the whole CI job. Cap at MAX_OUTPUT_LINES with a clear
# TRUNCATED marker so the limit is visible, not silent.
MAX_OUTPUT_LINES=${MAX_OUTPUT_LINES:-50000}

# Cap a captured-string output to MAX_OUTPUT_LINES, appending a
# TRUNCATED marker if the cap fired. Linear-time — uses awk's
# line-counting + cutoff, never re-walks the input.
cap_output() {
    awk -v cap="$MAX_OUTPUT_LINES" '
        { lines++ }
        lines <= cap { print; next }
        END {
            if (lines > cap) {
                print "TRUNCATED at " cap " lines (total: " lines ")"
            }
        }
    '
}

# Function to normalize output for comparison
normalize_output() {
    local input="$1"

    # Issue #796 — first pass (Buffer-line decode) used to be a bash
    # while-read loop with `decoded+="$line"\n` per iteration. That's
    # O(n²) on input size: 5.7M lines × 2.85M-char-average tail ≈ 16T
    # bytes of string concatenation, which burned ~3 hours on CI before
    # the runner was killed. Replaced with a single python3 pass —
    # linear time, decodes `<Buffer XX XX ...>` to its UTF-8 bytes in
    # one walk. python3 is preinstalled on every ubuntu/macos runner.
    #
    # The decode is bytes-faithful: invalid UTF-8 sequences become U+FFFD
    # via `errors="replace"`, matching the pre-fix `xxd -r -p` behavior
    # for arbitrary binary content.
    local decoded
    decoded=$(printf '%s' "$input" | python3 -c '
import sys
for raw in sys.stdin:
    line = raw.rstrip("\n").rstrip("\r")
    if line.startswith("<Buffer ") and line.endswith(">"):
        hex_part = line[len("<Buffer "):-1].replace(" ", "")
        try:
            sys.stdout.write(bytes.fromhex(hex_part).decode("utf-8", errors="replace"))
            sys.stdout.write("\n")
        except ValueError:
            # Not a valid hex sequence — pass through unchanged so the
            # diff still pinpoints the divergence.
            print(line)
    else:
        print(line)
')

    echo "$decoded" | \
        # Normalize line endings
        tr -d '\r' | \
        # Strip Node v22+ MODULE_TYPELESS_PACKAGE_JSON warnings (4 lines
        # printed to stderr when running .ts files without "type":
        # "module" in package.json — pure environmental noise that
        # appeared after the Node v25 upgrade and has nothing to do
        # with Perry's output).
        sed -E '/^\(node:[0-9]+\) \[MODULE_TYPELESS_PACKAGE_JSON\]/d' | \
        sed -E '/^\(node:[0-9]+\)( \[[^]]+\])? DeprecationWarning:/d' | \
        sed -E '/^\(node:[0-9]+\) ExperimentalWarning: Type Stripping is an experimental feature/d' | \
        sed -E '/^\(node:[0-9]+\) ExperimentalWarning: glob is an experimental feature/d' | \
        sed -E '/^\(node:[0-9]+\) ExperimentalWarning: WASI is an experimental feature/d' | \
        sed -E '/^\(node:[0-9]+\) Warning: tracePromise was called with the function .* returned a non-thenable\.$/d' | \
        sed -E '/^Debugger listening on ws:\/\/[^[:space:]]+$/d' | \
        sed -E '/^For help, see: https:\/\/nodejs\.org\/en\/docs\/inspector$/d' | \
        sed -E 's/^\(node:[0-9]+\) (Timeout(Overflow|Negative|NaN)Warning:)/(node:<pid>) \1/' | \
        sed -E '/^Timeout duration was set to [0-9]+\.$/d' | \
        sed -E '/^\(Use `node --trace-deprecation/d' | \
        sed -E '/^Reparsing as ES module because module syntax was detected/d' | \
        sed -E '/^To eliminate this warning, add "type": "module"/d' | \
        sed -E '/^\(Use `node --trace-warnings/d' | \
        # Trim trailing whitespace on each line
        sed 's/[[:space:]]*$//' | \
        # Normalize boolean output: true->1, false->0 (whole line only)
        sed -E 's/^true$/1/' | \
        sed -E 's/^false$/0/' | \
        # Normalize floating point precision (keep 10 decimal places)
        sed -E 's/([0-9]+\.[0-9]{10})[0-9]+/\1/g' | \
        # Normalize console.time/timeLog/timeEnd output: the elapsed value
        # will always differ between Node.js (JIT) and Perry (native LLVM).
        # Covers ms, s, and μs (microseconds — emitted on fast macOS-14 ARM
        # runners when timer duration is < 1 ms, e.g. timerA/timerB in
        # test_gap_console_methods which have no work between start and end).
        # The optional [[:space:]]* handles Node.js v18's "N.NNN ms" format
        # (space before unit); v22 produces "N.NNNms" without.
        # The first pass also covers console.timeLog(label, ...data), where
        # Node prints extra payload after the duration.
        sed -E 's/^([^:]*): [0-9]+(\.[0-9]+)?[[:space:]]*(μs|ms|s)( .*)$/\1: <timer>\4/g' | \
        sed -E 's/^([^:]*): [0-9]+(\.[0-9]+)?[[:space:]]*(μs|ms|s)$/\1: <timer>/g' | \
        # Normalize node:test's measured durations in the default reporter.
        sed -E 's/^([✔✖﹣] .*) \([0-9]+(\.[0-9]+)?ms\)( .*)$/\1 (<duration>)\3/g' | \
        sed -E 's/^([✔✖﹣] .*) \([0-9]+(\.[0-9]+)?ms\)$/\1 (<duration>)/g' | \
        sed -E 's/^ℹ duration_ms [0-9]+(\.[0-9]+)?$/ℹ duration_ms <duration>/g' | \
        # Normalize console warning delivery: Node emits process warnings on
        # stderr after the script body, while Perry writes the equivalent
        # warning eagerly at the call site.
        sed -E '/^(\(node:[0-9]+\) )?Warning: (Count for .* does not exist|No such label .* for console\.(timeLog|timeEnd)\(\)|Label .* already exists for console\.time\(\))/d' | \
        # Normalize Node-style process warning prefixes. The warning text is
        # semantically relevant, but the pid is not stable across runs.
        sed -E 's/^\(node:[0-9]+\) /\(node:<pid>\) /g' | \
        # Normalize console.trace output: strip stack frame lines so only
        # the "Trace: <message>" header survives for comparison.
        # Node.js emits "    at <symbol> (<location>)" JS stack frames;
        # Perry emits "    N: <symbol>" native frame headers, indented
        # "             at <file:line>" continuation lines, and
        # "        (… N more identical frames)" dedup-collapse lines.
        # All three shapes have leading whitespace; the distinguishing
        # suffixes are "at ", a digit+colon, or a literal "(…".
        sed -E '/^[[:space:]]+at /d' | \
        sed -E '/^[[:space:]]+[0-9]+: /d' | \
        sed -E '/^[[:space:]]+[(].*more identical frames[)]/d' | \
        # Remove trailing empty lines
        sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'
}

echo "========================================"
echo "   Perry Parity Test Runner ($BACKEND_LABEL)"
echo "========================================"
echo ""

# Build the compiler + runtime + stdlib in release mode. We invoke the
# resulting `target/release/perry` binary directly per-test below — pre-fix
# the loop ran `cargo run --quiet --bin perry` which (a) silently triggers a
# *debug* build of perry that's slower at compile-time and runtime than the
# release binary the prior step had just produced, and (b) adds cargo's own
# per-invocation overhead × ~150 tests.
TARGET_DIR="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}"
PERRY_BIN="$TARGET_DIR/release/perry"
echo "Building compiler (release)..."
BUILD_PACKAGES=(-p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static)
BUILD_FEATURES=()
needs_wasm_host=0
if [[ -n "${PERRY_NO_AUTO_OPTIMIZE:-}" && "$TEST_SUITE" == "node-suite" ]]; then
    case "$MODULE_FILTER" in
        ""|http|http/*|https|https/*|http2|http2/*)
            # No-auto optimized links still consume prebuilt well-known ext archives
            # and need the matching stdlib pump hooks compiled into libperry_stdlib.a.
            # HTTP fixtures can also emit net + ws well-known owners via the codegen
            # FFI registry, so build those wrappers too (#4373).
            BUILD_PACKAGES+=(-p perry-ext-http -p perry-ext-net -p perry-ext-ws)
            BUILD_FEATURES+=(perry-stdlib/external-http-server-pump perry-stdlib/external-http-client-pump)
            ;;
    esac
    case "$MODULE_FILTER" in
        ""|zlib|zlib/*)
            # zlib no-auto node-suite runs still route `node:zlib` through the
            # well-known external archive, so build that archive and the stdlib
            # bridge that drains its stream queue and dispatches stream handles.
            BUILD_PACKAGES+=(-p perry-ext-zlib)
            BUILD_FEATURES+=(perry-stdlib/external-zlib-pump)
            ;;
    esac
fi
needs_wasm_host=0
if [[ "$TEST_SUITE" == "node-suite" ]]; then
    case "$MODULE_FILTER" in
        ""|globals|globals/*)
            if [[ -z "$TEST_FILTER" || "$TEST_FILTER" == *webassembly* || "$TEST_FILTER" == *wasm* ]]; then
                needs_wasm_host=1
            fi
            ;;
    esac
    case "$MODULE_FILTER:$TEST_FILTER" in
        :|:webassembly*|:*webassembly*|globals:|globals:webassembly*|globals:*webassembly*|globals/*:|globals/*:webassembly*|globals/*:*webassembly*)
            # WebAssembly metadata fixtures lower to wasm-host runtime calls. In
            # no-auto mode, build the feature-enabled runtime and host archive
            # up front so compile/link matches the auto-detected path.
            needs_wasm_host=1
            ;;
    esac
fi
needs_ext_net=0
if [[ "$TEST_SUITE" == "node-suite" ]]; then
    case "$MODULE_FILTER" in
        ""|net|net/*)
            # node-suite/net commonly runs with PERRY_NO_AUTO_OPTIMIZE=1.
            # That path links prebuilt well-known archives, so build ext-net
            # once up front instead of failing on unresolved js_net_* symbols.
            needs_ext_net=1
            ;;
    esac
fi
BUILD_FEATURE_ARGS=()
if [[ "${#BUILD_FEATURES[@]}" -gt 0 ]]; then
    feature_csv=$(IFS=,; echo "${BUILD_FEATURES[*]}")
    BUILD_FEATURE_ARGS=(--features "$feature_csv")
fi
if ! cargo build --release --quiet "${BUILD_PACKAGES[@]}" "${BUILD_FEATURE_ARGS[@]}" 2>/dev/null; then
    echo -e "${RED}Failed to build compiler/runtime archives${NC}"
    exit 1
fi
if [[ "$needs_wasm_host" -eq 1 ]]; then
    # WebAssembly metadata fixtures exercise the real host shims. Build the
    # wasm-enabled runtime staticlib after the CLI build above; enabling this
    # feature while building the `perry` binary would make the CLI link against
    # unresolved perry_wasm_host_* symbols.
    echo "Building WebAssembly host runtime (release)..."
    if ! cargo build --release --quiet -p perry-runtime-static -p perry-wasm-host --features perry-runtime/wasm-host 2>/dev/null; then
        echo -e "${RED}Failed to build WebAssembly host runtime archives${NC}"
        exit 1
    fi
fi
if [[ "$needs_ext_net" -eq 1 ]]; then
    echo "Building net extension (release)..."
    ext_net_jobs="${CARGO_BUILD_JOBS:-1}"
    if ! cargo build --release --quiet -p perry-ext-net -j "$ext_net_jobs" 2>/dev/null; then
        echo -e "${RED}Failed to build net extension library${NC}"
        exit 1
    fi
fi
if [[ ! -x "$PERRY_BIN" ]]; then
    echo -e "${RED}Expected $PERRY_BIN after release build${NC}"
    exit 1
fi

echo -e "${GREEN}Compiler and runtime archives built successfully${NC}"
echo ""
echo "Running parity tests (backend: $BACKEND_LABEL, suite: $TEST_SUITE${MODULE_FILTER:+, module: $MODULE_FILTER})..."
echo ""

# ---------------------------------------------------------------------------
# Echo server lifecycle (port 17891) — required by test_net_min and
# test_net_socket.  We spawn it in the background, wait up to 5 s for it to
# accept connections, and register a cleanup trap so it always gets killed on
# script exit regardless of how the script terminates.
# ---------------------------------------------------------------------------
ECHO_SERVER_PID=""
ECHO_SERVER_SCRIPT="$SCRIPT_DIR/test-files/test_net_echo_server.py"

start_echo_server() {
    if ! command -v python3 &>/dev/null; then
        echo "Warning: python3 not found — test_net_min / test_net_socket will fail parity"
        return
    fi
    if [[ ! -f "$ECHO_SERVER_SCRIPT" ]]; then
        echo "Warning: $ECHO_SERVER_SCRIPT not found — test_net_min / test_net_socket will fail parity"
        return
    fi
    python3 "$ECHO_SERVER_SCRIPT" &
    ECHO_SERVER_PID=$!
    # Poll up to 5 s (50 × 100 ms) for the server to accept connections.
    local ready=0
    if wait_for_tcp_port 127.0.0.1 17891 50 0.1; then
        ready=1
    fi
    if [[ $ready -eq 1 ]]; then
        echo "Echo server started on 127.0.0.1:17891 (PID $ECHO_SERVER_PID)"
    else
        echo "Warning: echo server did not become ready in 5 s — net tests may fail parity"
    fi
}

stop_echo_server() {
    if [[ -n "$ECHO_SERVER_PID" ]]; then
        kill "$ECHO_SERVER_PID" 2>/dev/null
        wait "$ECHO_SERVER_PID" 2>/dev/null
        ECHO_SERVER_PID=""
    fi
}

trap stop_echo_server EXIT

# The granular node-suite starts with deterministic module cases (path, url,
# etc.) that do not need the legacy top-level net echo server. Future net
# node-suite cases can opt into their own per-test companion lifecycle.
if [[ "$TEST_SUITE" != "node-suite" ]]; then
    start_echo_server
fi
echo ""

# JSON report data
REPORT_FILE="$REPORT_DIR/parity_report_$(date +%Y%m%d_%H%M%S).json"
LATEST_REPORT="$REPORT_DIR/latest.json"

# Compact per-test records consumed by scripts/parity_matrix_trend.py.
declare -a TEST_RESULTS=()

record_result() {
    local test_id=$1
    local status=$2
    TEST_RESULTS+=("{\"id\":\"$test_id\",\"status\":\"$status\"}")
}

declare -a TEST_FILES=()
case "$TEST_SUITE" in
    all)
        while IFS= read -r test_file; do
            TEST_FILES+=("$test_file")
        done < <(find "$TEST_DIR" -maxdepth 1 -type f -name '*.ts' | sort)
        ;;
    parity|smoke)
        while IFS= read -r test_file; do
            TEST_FILES+=("$test_file")
        done < <(find "$TEST_DIR" -maxdepth 1 -type f -name 'test_parity_*.ts' | sort)
        ;;
    node-suite)
        node_suite_search_root="$NODE_SUITE_DIR"
        if [[ -n "$MODULE_FILTER" ]]; then
            node_suite_search_root="$NODE_SUITE_DIR/$MODULE_FILTER"
        fi
        if [[ -d "$node_suite_search_root" ]]; then
            while IFS= read -r test_file; do
                TEST_FILES+=("$test_file")
            done < <(find "$node_suite_search_root" -type f -name '*.ts' | sort)
        fi
        ;;
    *)
        echo -e "${RED}Unknown suite: $TEST_SUITE${NC}"
        echo "Known suites: all, parity, smoke, node-suite"
        exit 1
        ;;
esac

if [[ ${#TEST_FILES[@]} -eq 0 ]]; then
    echo -e "${YELLOW}No tests matched suite/filter selection${NC}"
fi

# Run each test
for test_file in "${TEST_FILES[@]}"; do
    # Skip directories (multi/ folder)
    [[ -d "$test_file" ]] && continue

    test_name=$(basename "$test_file" .ts)
    if [[ "$test_file" == "$NODE_SUITE_DIR"/* ]]; then
        test_rel="${test_file#"$NODE_SUITE_DIR"/}"
        test_id="node-suite/${test_rel%.ts}"
    else
        test_id="$test_name"
    fi

    # Optional --filter flag: only run tests whose basename or suite id
    # contains it.
    if [[ -n "$TEST_FILTER" ]] && [[ "$test_name" != *"$TEST_FILTER"* ]] && [[ "$test_id" != *"$TEST_FILTER"* ]]; then
        continue
    fi

    safe_test_id="${test_id//\//__}"
    node_output_file="$OUTPUT_DIR/node/${safe_test_id}.txt"
    perry_output_file="$OUTPUT_DIR/perry/${safe_test_id}.txt"
    perry_binary="$PARITY_TMP/perry_parity_$safe_test_id"
    parity_argv_line=$(sed -n -E 's|^[[:space:]]*//[[:space:]]*parity-argv:[[:space:]]*(.*)$|\1|p' "$test_file" | head -1)
    parity_node_argv_line=$(sed -n -E 's|^[[:space:]]*//[[:space:]]*parity-node-argv:[[:space:]]*(.*)$|\1|p' "$test_file" | head -1)
    parity_env_line=$(sed -n -E 's|^[[:space:]]*//[[:space:]]*parity-env:[[:space:]]*(.*)$|\1|p' "$test_file" | head -1)
    test_argv=()
    if [[ -n "$parity_argv_line" ]]; then
        read -r -a test_argv <<< "$parity_argv_line"
    fi
    node_argv=()
    if [[ -n "$parity_node_argv_line" ]]; then
        read -r -a node_argv <<< "$parity_node_argv_line"
    fi
    parity_env=()
    if [[ -n "$parity_env_line" ]]; then
        read -r -a parity_env <<< "$parity_env_line"
    fi

    # Check if test should be skipped
    if should_skip "$test_name"; then
        echo -e "${YELLOW}SKIP${NC}  $test_id (async/timer test)"
        ((SKIPPED++))
        record_result "$test_id" "skipped"
        continue
    fi

    # Spawn per-test companion servers when needed.
    # test_net_upgrade_tls* — plain→TLS upgrade server on port 17892 (issue #275).
    local_server_pid=""
    if [[ "$test_name" == test_net_upgrade_tls* ]]; then
        start_tls_upgrade_server
        local_server_pid="$TLS_UPGRADE_SERVER_PID"
    fi

    # Run with Node.js. Stream stdout/stderr to a temp file first, then
    # cap before reading into bash (#796): a pathological test that
    # emits millions of lines would otherwise blow up command-substitution
    # memory and DOS the runner. PIPESTATUS doesn't propagate across
    # `$(...)`, so capturing the exit code requires the file detour
    # rather than a `cmd | cap_output` pipeline.
    node_tmp=$(mktemp)
    run_with_timeout 10 env FORCE_COLOR=0 NO_COLOR=1 NODE_DISABLE_COLORS=1 \
        node --experimental-strip-types "${node_argv[@]}" "$test_file" "${test_argv[@]}" > "$node_tmp" 2>&1
    node_exit=$?
    node_output=$(cap_output < "$node_tmp")
    rm -f "$node_tmp"

    if [[ $node_exit -ne 0 && $node_exit -ne 124 ]]; then
        # Node.js failed — if we have a stored expected-output file for this
        # test (e.g. because the test uses syntax that this Node version
        # doesn't support, like `await using` on Node <22.12), fall through
        # to compile+run Perry and compare against the expected file.
        # Otherwise record NODE_FAIL and skip.
        if ! has_expected_output "$test_name"; then
            echo -e "${YELLOW}SKIP${NC}  $test_id (Node.js error: exit $node_exit)"
            ((NODE_FAIL++))
            record_result "$test_id" "node_fail"
            [[ -n "$local_server_pid" ]] && stop_tls_upgrade_server
            continue
        fi
        echo -e "${YELLOW}NOTE${NC}  $test_id (Node.js error: exit $node_exit — using expected-output)"
    fi

    # Save Node.js output
    echo "$node_output" > "$node_output_file"

    # Compile with Perry. test_parity_* files run in permissive mode
    # (PERRY_ALLOW_UNIMPLEMENTED=1) so unimplemented APIs surface as
    # runtime divergence (the gap signal) instead of hard compile errors.
    compile_env=""
    if [[ "$test_name" == test_parity_* || "$test_id" == node-suite/* ]]; then
        compile_env="PERRY_ALLOW_UNIMPLEMENTED=1"
    fi
    compile_flags=()
    if [[ -n "$BACKEND_FLAG" ]]; then
        compile_flags+=("$BACKEND_FLAG")
    fi
    if [[ "$needs_wasm_host" -eq 1 && ( "$test_id" == node-suite/*/*webassembly* || "$test_id" == node-suite/*/*wasm* ) ]]; then
        compile_flags+=(--enable-wasm-runtime)
    fi
    # #499: some parity tests transitively pull in `.js` fixtures
    # (jsruntime/* tests by design, plus a long-tail of others: V8
    # fallback fixtures, js_interop callbacks, nest_js_common decorators,
    # etc.). The host-opt-in gate refuses linkage by default. Mirror the
    # compile-smoke retry pattern: try once without the flag (keeps
    # native-only binaries cheap and surfaces tests that *shouldn't*
    # be pulling QuickJS in), and if the error names `perry-jsruntime`,
    # retry once with `--enable-js-runtime`. Avoids hand-curating a list
    # of test names that need V8.
    compile_output=$(env $compile_env "${parity_env[@]}" "$PERRY_BIN" "${compile_flags[@]}" "$test_file" -o "$perry_binary" 2>&1)
    compile_exit=$?
    if [[ $compile_exit -ne 0 ]] && grep -q "perry-jsruntime" <<<"$compile_output"; then
        compile_output=$(env $compile_env "${parity_env[@]}" "$PERRY_BIN" "${compile_flags[@]}" --enable-js-runtime "$test_file" -o "$perry_binary" 2>&1)
        compile_exit=$?
    fi

    if [[ $compile_exit -ne 0 ]]; then
        echo -e "${RED}FAIL${NC}  $test_id (compile error)"
        ((COMPILE_FAIL++))
        COMPILE_FAILURES+=("$test_id")
        record_result "$test_id" "compile_fail"
        echo "" > "$perry_output_file"
        # Persist the actual compile stderr so CI artifacts can be inspected
        # to diagnose long-tail compile failures (e.g. the macOS-14 SDK gap
        # tracked as `ci-env` in test-parity/known_failures.json). Pre-fix
        # the parity runner only logged "compile error" with no detail and
        # the macOS-14 family was diagnosed by inference, not data.
        compile_log="$OUTPUT_DIR/${safe_test_id}.compile_error.log"
        printf "%s\n" "$compile_output" > "$compile_log"
        [[ -n "$local_server_pid" ]] && stop_tls_upgrade_server
        continue
    fi

    # Run Perry binary — same cap-via-tempfile protocol as Node above (#796).
    # Silence the stub-elimination first-call diagnostics (#4919): parity
    # measures *behavioral* byte-parity against Node, and Node never emits
    # Perry's `[perry] warning: ... is a stub` lines. They'd otherwise show
    # up on the 2>&1-captured stream for any test that exercises a flagged
    # API (dns/dgram loopback, v8 heap snapshot, …) and diff against Node.
    perry_tmp=$(mktemp)
    run_with_timeout 10 env PERRY_STUB_DIAG=off "${parity_env[@]}" "$perry_binary" "${test_argv[@]}" > "$perry_tmp" 2>&1
    perry_exit=$?
    perry_output=$(cap_output < "$perry_tmp")
    rm -f "$perry_tmp"

    # Save Perry output
    echo "$perry_output" > "$perry_output_file"

    # For tests that have a stored expected-output file (Perry-specific APIs
    # that don't map 1:1 to Node.js), compare Perry output against the file
    # instead of against Node.js.  This lets us verify Perry's behaviour
    # end-to-end without requiring Node.js to speak the same API.
    if has_expected_output "$test_name"; then
        expected_exit=$(expected_exit_code "$test_name")
        expected_normalized=$(normalize_output "$(cat "$EXPECTED_DIR/${test_name}.txt")")
        perry_normalized=$(normalize_output "$perry_output")
        if [[ "$perry_exit" == "$expected_exit" && "$perry_normalized" == "$expected_normalized" ]]; then
            echo -e "${GREEN}PASS${NC}  $test_id (expected-output)"
            ((PARITY_PASS++))
            status="pass"
        else
            echo -e "${RED}FAIL${NC}  $test_id (expected-output mismatch)"
            ((PARITY_FAIL++))
            PARITY_FAILURES+=("$test_id")
            status="parity_fail"
            echo "       Expected exit: $expected_exit"
            echo "       Perry exit:    $perry_exit"
            echo "       Expected: $(cat "$EXPECTED_DIR/${test_name}.txt" | head -1)"
            echo "       Perry:    $(echo "$perry_output" | head -1)"
        fi
    else
        # Normalize both outputs for comparison
        node_normalized=$(normalize_output "$node_output")
        perry_normalized=$(normalize_output "$perry_output")

        # Compare outputs
        if [[ "$node_normalized" == "$perry_normalized" ]]; then
            echo -e "${GREEN}PASS${NC}  $test_id"
            ((PARITY_PASS++))
            status="pass"
        else
            echo -e "${RED}FAIL${NC}  $test_id (output mismatch)"
            ((PARITY_FAIL++))
            PARITY_FAILURES+=("$test_id")
            status="parity_fail"

            # Show diff for failures (first few lines)
            echo "       Node.js:    $(echo "$node_output" | head -1)"
            echo "       Perry:  $(echo "$perry_output" | head -1)"
        fi
    fi

    record_result "$test_id" "$status"

    # Stop any per-test companion server that was started for this test.
    [[ -n "$local_server_pid" ]] && stop_tls_upgrade_server

    # Clean up binary
    rm -f "$perry_binary"
done

# Calculate parity percentage
TOTAL_RUN=$((PARITY_PASS + PARITY_FAIL))
if [[ $TOTAL_RUN -gt 0 ]]; then
    PARITY_PCT=$(echo "scale=1; $PARITY_PASS * 100 / $TOTAL_RUN" | bc)
else
    PARITY_PCT="0.0"
fi

# Summary
echo ""
echo "========================================"
echo "   Parity Test Summary"
echo "========================================"
echo -e "${GREEN}Parity Pass:${NC}   $PARITY_PASS"
echo -e "${RED}Parity Fail:${NC}   $PARITY_FAIL"
echo -e "${RED}Compile Fail:${NC}  $COMPILE_FAIL"
echo -e "${YELLOW}Skipped:${NC}       $SKIPPED"
echo ""
echo -e "${CYAN}Parity Rate:${NC}   ${PARITY_PCT}%"
echo ""

# List failures
if [[ ${#PARITY_FAILURES[@]} -gt 0 ]]; then
    echo "Output Mismatches:"
    for failed in "${PARITY_FAILURES[@]}"; do
        echo "  - $failed"
    done
    echo ""
fi

if [[ ${#COMPILE_FAILURES[@]} -gt 0 ]]; then
    echo "Compile Failures:"
    for failed in "${COMPILE_FAILURES[@]}"; do
        echo "  - $failed"
    done
    echo ""
fi

RESULTS_JSON=$(printf '%s\n' "${TEST_RESULTS[@]}" | paste -sd, -)

# Generate JSON report
cat > "$REPORT_FILE" << EOF
{
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "summary": {
    "parity_pass": $PARITY_PASS,
    "parity_fail": $PARITY_FAIL,
    "compile_fail": $COMPILE_FAIL,
    "node_fail": $NODE_FAIL,
    "skipped": $SKIPPED,
    "total_run": $TOTAL_RUN,
    "parity_percentage": $PARITY_PCT
  },
  "failures": {
    "parity": [$(printf '"%s",' "${PARITY_FAILURES[@]}" | sed 's/,$//')]
,
    "compile": [$(printf '"%s",' "${COMPILE_FAILURES[@]}" | sed 's/,$//')]

  },
  "results": [${RESULTS_JSON}]
}
EOF

# Create latest symlink
cp "$REPORT_FILE" "$LATEST_REPORT"

echo "Report saved to: $REPORT_FILE"
echo ""

# release_sweep.sh consumes a flat single-line summary if PERRY_TEST_SUMMARY_OUT
# is exported. Standalone runs (env var unset) are unaffected.
if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_parity_tests.sh", "passed": $PARITY_PASS, "failed": $((PARITY_FAIL + COMPILE_FAIL)), "skipped": $SKIPPED, "total": $TOTAL_RUN, "rate_pct": $PARITY_PCT}
EOF
fi

# Exit with error if parity is below threshold (80%)
if (( $(echo "$PARITY_PCT < 80" | bc -l) )); then
    echo -e "${RED}Parity below 80% threshold${NC}"
    exit 1
fi
