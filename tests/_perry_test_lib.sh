# Shared harness for the tests/test_*.sh regression suite.
#
# SOURCED, never executed — the leading underscore keeps it out of
# run_tests.sh's `tests/test_*.sh` glob (same convention as the fixture
# harness's _fixture_lib.sh). It centralizes the perry-binary + runtime-lib
# detection that every regression test would otherwise copy-paste, so a change
# to the compile invocation lives in one place instead of ~130 files.
#
# Usage — single source file, literal-oracle:
#   source "$(dirname "$0")/_perry_test_lib.sh"
#   perry_run main.ts <<'TS'
#   console.log("hi");
#   TS
#   perry_expect 'hi'
#   perry_pass "greeting"
#
# Usage — compare against Node instead of a literal:
#   perry_run main.ts <<'TS' ... TS
#   perry_expect_node          # SKIPs cleanly if node is absent
#
# Usage — multi-module: write extra files first, then compile the entry:
#   perry_write dep.ts <<'TS' ... TS
#   perry_run main.ts <<'TS' ... TS   # main.ts imports ./dep

set -euo pipefail

_PT_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[1]}")" && pwd)"
REPO_ROOT="$(cd "$_PT_SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
[[ -x "$PERRY" ]] || PERRY="$REPO_ROOT/target/debug/perry"
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

_PT_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_PT_TMPDIR"' EXIT
_PT_SRC=""

# perry_write <name> : write a source file into the temp dir from stdin.
perry_write() {
    cat > "$_PT_TMPDIR/$1"
}

# perry_run <entry> : write <entry> from stdin, then compile and run it,
# capturing stdout to run.log. Write any extra modules with perry_write first.
perry_run() {
    _PT_SRC="$_PT_TMPDIR/$1"
    cat > "$_PT_SRC"
    local bin="$_PT_TMPDIR/out"
    local args=(compile --no-cache)
    # Link the prebuilt static libs only from the SAME target dir as the
    # selected perry binary, so a release binary never pairs with debug libs
    # (or vice versa). If that dir has no libs, leave PERRY_RUNTIME_DIR unset
    # and let perry build/link its own runtime.
    local perry_dir; perry_dir="$(cd "$(dirname "$PERRY")" && pwd)"
    if [[ -f "$perry_dir/libperry_runtime.a" && -f "$perry_dir/libperry_stdlib.a" ]]; then
        export PERRY_RUNTIME_DIR="$perry_dir"; args+=(--no-auto-optimize)
    fi
    "$PERRY" "${args[@]}" "$_PT_SRC" -o "$bin" > "$_PT_TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"; sed 's/^/    /' "$_PT_TMPDIR/compile.log" | tail -80; exit 1
    }
    "$bin" > "$_PT_TMPDIR/run.log" 2>&1 || {
        echo "FAIL: program failed"; sed 's/^/    /' "$_PT_TMPDIR/run.log" | tail -80; exit 1
    }
}

# perry_expect [literal] : diff run.log against the expected output. With an
# argument, expects that single line; with no argument, reads the expected
# (possibly multi-line) output from stdin — e.g. perry_expect <<'EOF' ... EOF.
perry_expect() {
    if [[ $# -gt 0 ]]; then
        printf '%s\n' "$1" > "$_PT_TMPDIR/expected.log"
    else
        cat > "$_PT_TMPDIR/expected.log"
    fi
    diff -u "$_PT_TMPDIR/expected.log" "$_PT_TMPDIR/run.log" || { echo "FAIL: output mismatch"; exit 1; }
}

# perry_expect_node : diff run.log against `node <entry>`; SKIP if node absent.
perry_expect_node() {
    command -v node >/dev/null 2>&1 || { echo "SKIP: node binary not found"; exit 0; }
    node "$_PT_SRC" > "$_PT_TMPDIR/expected.log"
    diff -u "$_PT_TMPDIR/expected.log" "$_PT_TMPDIR/run.log" || { echo "FAIL: output mismatch vs node"; exit 1; }
}

perry_pass() { echo "PASS: $1"; }
