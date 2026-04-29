#!/bin/bash
# Perry Parity Test Runner
# Compares output between Node.js and Perry native compilation

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEST_DIR="$SCRIPT_DIR/test-files"
OUTPUT_DIR="$SCRIPT_DIR/test-parity/output"
REPORT_DIR="$SCRIPT_DIR/test-parity/reports"

# LLVM is the only backend post-Phase K hard cutover. The --llvm /
# --cranelift flags and PERRY_BACKEND env var are kept as no-ops for
# backward compat with existing scripts.
BACKEND_FLAG=""
BACKEND_LABEL="LLVM"

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

# Counters
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

# Tests to skip (async tests that hang, random-dependent tests, etc.)
SKIP_TESTS=(
    # Async tests (need event loop)
    "test_async"
    "test_async2"
    "test_async3"
    "test_async4"
    "test_async5"
    "test_async_chain"
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
    # Network tests — require a live TCP/TLS server on the host. CI runners
    # don't host one, so these consistently fail with `Connection refused`
    # against any hard-coded address. Skip in environments where the
    # server isn't available; both Perry and Node hit the same error so
    # the diff is just network-noise.
    "test_net_min"
    "test_net_socket"
    "test_net_upgrade_tls"
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

# Function to normalize output for comparison
normalize_output() {
    local input="$1"

    # First pass: decode Buffer representations
    # <Buffer XX XX...> -> decoded string
    local decoded=""
    while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ "$line" == "<Buffer"* ]]; then
            # Extract hex part and decode
            local hex=$(echo "$line" | sed 's/<Buffer //' | sed 's/>//')
            # Decode hex to string (may contain embedded newlines)
            local decoded_line=$(echo "$hex" | xxd -r -p)
            decoded+="$decoded_line"$'\n'
        else
            decoded+="$line"$'\n'
        fi
    done <<< "$input"

    echo "$decoded" | \
        # Normalize line endings
        tr -d '\r' | \
        # Strip Node v22+ MODULE_TYPELESS_PACKAGE_JSON warnings (4 lines
        # printed to stderr when running .ts files without "type":
        # "module" in package.json — pure environmental noise that
        # appeared after the Node v25 upgrade and has nothing to do
        # with Perry's output).
        sed -E '/^\(node:[0-9]+\) \[MODULE_TYPELESS_PACKAGE_JSON\]/d' | \
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
        # Strip the numeric portion, keeping only label and unit so both
        # sides compare as "label: <timer>".
        sed -E 's/^([^:]+): [0-9]+(\.[0-9]+)?(ms|s)$/\1: <timer>/g' | \
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
PERRY_BIN="$SCRIPT_DIR/target/release/perry"
echo "Building compiler (release)..."
if ! cargo build --release --quiet -p perry -p perry-runtime -p perry-stdlib 2>/dev/null; then
    echo -e "${RED}Failed to build compiler${NC}"
    exit 1
fi
if [[ ! -x "$PERRY_BIN" ]]; then
    echo -e "${RED}Expected $PERRY_BIN after release build${NC}"
    exit 1
fi

echo -e "${GREEN}Compiler and runtime built successfully${NC}"
echo ""
echo "Running parity tests (backend: $BACKEND_LABEL)..."
echo ""

# JSON report data
REPORT_FILE="$REPORT_DIR/parity_report_$(date +%Y%m%d_%H%M%S).json"
LATEST_REPORT="$REPORT_DIR/latest.json"

# Start JSON array for test results
TEST_RESULTS="[]"

# Run each test
for test_file in "$TEST_DIR"/*.ts; do
    # Skip directories (multi/ folder)
    [[ -d "$test_file" ]] && continue

    test_name=$(basename "$test_file" .ts)
    node_output_file="$OUTPUT_DIR/node/${test_name}.txt"
    perry_output_file="$OUTPUT_DIR/perry/${test_name}.txt"
    perry_binary="/tmp/perry_parity_$test_name"

    # Check if test should be skipped
    if should_skip "$test_name"; then
        echo -e "${YELLOW}SKIP${NC}  $test_name (async/timer test)"
        ((SKIPPED++))
        continue
    fi

    # Run with Node.js
    node_output=$(run_with_timeout 10 node --experimental-strip-types "$test_file" 2>&1)
    node_exit=$?

    if [[ $node_exit -ne 0 && $node_exit -ne 124 ]]; then
        # Node.js failed - might be expected for some tests
        echo -e "${YELLOW}SKIP${NC}  $test_name (Node.js error: exit $node_exit)"
        ((NODE_FAIL++))
        continue
    fi

    # Save Node.js output
    echo "$node_output" > "$node_output_file"

    # Compile with Perry. Direct invocation of the release binary built
    # above — pre-fix this was `cargo run --quiet --bin perry --` (no
    # `--release`), which silently triggered a debug build of perry that
    # was both slower as a compiler and incurred per-call cargo overhead
    # × ~150 tests. Direct binary call shaves multiple minutes off CI.
    compile_output=$("$PERRY_BIN" $BACKEND_FLAG "$test_file" -o "$perry_binary" 2>&1)
    compile_exit=$?

    if [[ $compile_exit -ne 0 ]]; then
        echo -e "${RED}FAIL${NC}  $test_name (compile error)"
        ((COMPILE_FAIL++))
        COMPILE_FAILURES+=("$test_name")
        echo "" > "$perry_output_file"
        # Persist the actual compile stderr so CI artifacts can be inspected
        # to diagnose long-tail compile failures (e.g. the macOS-14 SDK gap
        # tracked as `ci-env` in test-parity/known_failures.json). Pre-fix
        # the parity runner only logged "compile error" with no detail and
        # the macOS-14 family was diagnosed by inference, not data.
        compile_log="$OUTPUT_DIR/${test_name}.compile_error.log"
        printf "%s\n" "$compile_output" > "$compile_log"
        continue
    fi

    # Run Perry binary
    perry_output=$(run_with_timeout 10 "$perry_binary" 2>&1)
    perry_exit=$?

    # Save Perry output
    echo "$perry_output" > "$perry_output_file"

    # Normalize both outputs for comparison
    node_normalized=$(normalize_output "$node_output")
    perry_normalized=$(normalize_output "$perry_output")

    # Compare outputs
    if [[ "$node_normalized" == "$perry_normalized" ]]; then
        echo -e "${GREEN}PASS${NC}  $test_name"
        ((PARITY_PASS++))
        status="pass"
    else
        echo -e "${RED}FAIL${NC}  $test_name (output mismatch)"
        ((PARITY_FAIL++))
        PARITY_FAILURES+=("$test_name")
        status="fail"

        # Show diff for failures (first few lines)
        echo "       Node.js:    $(echo "$node_output" | head -1)"
        echo "       Perry:  $(echo "$perry_output" | head -1)"
    fi

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

  }
}
EOF

# Create latest symlink
cp "$REPORT_FILE" "$LATEST_REPORT"

echo "Report saved to: $REPORT_FILE"
echo ""

# Exit with error if parity is below threshold (80%)
if (( $(echo "$PARITY_PCT < 80" | bc -l) )); then
    echo -e "${RED}Parity below 80% threshold${NC}"
    exit 1
fi
