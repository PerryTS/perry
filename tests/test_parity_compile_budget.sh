#!/usr/bin/env bash
# #11560: which compile budget does run_parity_tests.sh give a fixture?
#
# An auto-optimize compile rebuilds a feature-stripped runtime+stdlib once per
# distinct feature set, whether or not the fixture routes a module to an ext
# wrapper, and that `cargo build` alone takes ~200-330s on hosted runners. The
# harness used to grant the toolchain budget only to ext-routed fixtures, so in
# the full tier (auto-optimize on for the whole corpus) every plain fixture that
# drew a new feature set was killed at the ordinary 300s budget and reported as
# a compile failure — 36 of the 09-27 run's 47 new failures.
#
# The mock compiler below sleeps longer than the ordinary budget and well
# inside the toolchain budget, so the verdict names the budget that applied:
#   - auto-optimize ON, plain fixture   -> toolchain budget -> PASS
#   - auto-optimize OFF, plain fixture  -> ordinary budget  -> compile TIMEOUT
#     (the fast tiers' hang bound must still bite)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="${PARITY_SCRIPT_UNDER_TEST:-$ROOT/run_parity_tests.sh}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cp "$SCRIPT" "$WORK/run_parity_tests.sh"
chmod +x "$WORK/run_parity_tests.sh"
mkdir -p "$WORK/bin" "$WORK/test-files" \
    "$WORK/test-parity/output/node" "$WORK/test-parity/output/perry" "$WORK/test-parity/reports"
# The full tier runs PERRY_SKIP_BUILD=0: the harness `cargo build`s and then
# uses $CARGO_TARGET_DIR/release/perry. The mock build is a no-op; the mock
# compiler is already in place.
cat > "$WORK/bin/cargo" <<'EOF'
#!/bin/sh
exit 0
EOF
# The mock compiler: a slow compile that then produces a working binary.
cat > "$WORK/perry" <<'EOF'
#!/bin/sh
sleep "${MOCK_COMPILE_SECONDS:-3}"
while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
        printf '#!/bin/sh\necho budget-ok\n' > "$2"
        chmod +x "$2"
        exit 0
    fi
    shift
done
exit 0
EOF
cat > "$WORK/bin/node" <<'EOF'
#!/bin/sh
echo budget-ok
EOF
cat > "$WORK/bin/ps" <<'EOF'
#!/bin/sh
echo 1
EOF
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) touch "$WORK/perry_runtime.lib" "$WORK/perry_stdlib.lib" ;;
    *) touch "$WORK/libperry_runtime.a" "$WORK/libperry_stdlib.a" ;;
esac
mkdir -p "$WORK/target/release"
# A plain fixture: imports nothing, so it routes to no ext wrapper and builds
# no WebAssembly host.
echo 'console.log("budget-ok");' > "$WORK/test-files/test_budget_plain.ts"
chmod +x "$WORK/bin/cargo" "$WORK/bin/node" "$WORK/bin/ps" "$WORK/perry"
cp "$WORK/perry" "$WORK/target/release/perry"
export PATH="$WORK/bin:$PATH"

fail() { echo "ASSERTION FAILED: $*" >&2; exit 1; }

run_harness() {
    set +e
    # `env -u` must precede the assignments, so the caller's args go first.
    env "$@" CARGO_TARGET_DIR="$WORK/target" \
        PERRY_COMPILE_TIMEOUT=1 PERRY_TOOLCHAIN_COMPILE_TIMEOUT=30 \
        MOCK_COMPILE_SECONDS=3 \
        "$WORK/run_parity_tests.sh" --filter test_budget_plain >"$WORK/output" 2>&1
    status=$?
    set -e
}

# The full tier: the harness builds, PERRY_NO_AUTO_OPTIMIZE is unset, so every
# compile may run a runtime+stdlib rebuild and gets the toolchain budget.
run_harness -u PERRY_NO_AUTO_OPTIMIZE PERRY_SKIP_BUILD=0
if ! grep -F "PASS" "$WORK/output" | grep -F "test_budget_plain" >/dev/null; then
    cat "$WORK/output" >&2
    fail "auto-optimize compile of a plain fixture was not given the toolchain budget (exit $status)"
fi
[[ "$status" -eq 0 ]] || { cat "$WORK/output" >&2; fail "expected a clean run, got exit $status"; }

# The fast tiers: PERRY_SKIP_BUILD=1 exports PERRY_NO_AUTO_OPTIMIZE=1, a plain
# compile links prebuilt archives, and the ordinary budget must still bound it.
run_harness PERRY_SKIP_BUILD=1 PERRY_BIN="$WORK/perry" PERRY_RUNTIME_DIR="$WORK"
if ! grep -F "compile TIMEOUT after 1s" "$WORK/output" >/dev/null; then
    cat "$WORK/output" >&2
    fail "no-auto compile of a plain fixture escaped the ordinary budget (exit $status)"
fi

echo "PASS"
