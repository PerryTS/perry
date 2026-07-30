#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cp "$ROOT/run_parity_tests.sh" "$WORK/run_parity_tests.sh"
mkdir -p "$WORK/bin" "$WORK/test-files" "$WORK/test-parity/node-suite/reuse" \
    "$WORK/test-parity/output/node" "$WORK/test-parity/output/perry" "$WORK/test-parity/reports"
cat > "$WORK/bin/cargo" <<'EOF'
#!/bin/sh
echo invoked >> "$CARGO_LOG"
exit 99
EOF
cat > "$WORK/perry" <<'EOF'
#!/bin/sh
echo "$0|$PERRY_RUNTIME_DIR|$PERRY_NO_AUTO_OPTIMIZE" >> "$PERRY_LOG"
while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
        cat > "$2" <<'BIN'
#!/bin/sh
echo reuse-ok
BIN
        chmod +x "$2"
        exit 0
    fi
    shift
done
exit 0
EOF
cat > "$WORK/bin/node" <<'EOF'
#!/bin/sh
echo reuse-ok
EOF
cat > "$WORK/bin/ps" <<'EOF'
#!/bin/sh
echo 1
EOF
touch "$WORK/test-parity/node-suite/reuse/basic.ts"
chmod +x "$WORK/bin/cargo" "$WORK/bin/node" "$WORK/bin/ps" "$WORK/perry"
export PATH="$WORK/bin:$PATH" CARGO_LOG="$WORK/cargo.log" PERRY_LOG="$WORK/perry.log"

run_failure() {
    local expected=$1
    shift
    if "$@" >"$WORK/output" 2>&1; then
        echo "expected failure: $expected" >&2
        exit 1
    fi
    grep -F "$expected" "$WORK/output" >/dev/null
}

run_failure "Invalid PERRY_SKIP_BUILD 'yes'" env PERRY_SKIP_BUILD=yes "$WORK/run_parity_tests.sh"
run_failure "PERRY_BIN is not executable" env PERRY_SKIP_BUILD=1 PERRY_BIN="$WORK/missing" "$WORK/run_parity_tests.sh"
mkdir -p "$WORK/empty"
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) touch "$WORK/empty/perry_runtime.lib" ;;
    *) touch "$WORK/empty/libperry_runtime.a" ;;
esac
run_failure "PERRY_RUNTIME_DIR must contain" env PERRY_SKIP_BUILD=1 PERRY_BIN="$WORK/perry" PERRY_RUNTIME_DIR="$WORK/empty" "$WORK/run_parity_tests.sh"

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) touch "$WORK/perry_runtime.lib" "$WORK/perry_stdlib.lib" ;;
    *) touch "$WORK/libperry_runtime.a" "$WORK/libperry_stdlib.a" ;;
esac
: > "$CARGO_LOG"
set +e
PERRY_SKIP_BUILD=1 PERRY_BIN="$WORK/perry" "$WORK/run_parity_tests.sh" --suite node-suite --module reuse >"$WORK/output" 2>&1
status=$?
set -e
[[ "$status" -eq 0 ]]
grep -F "Using prebuilt compiler: $WORK/perry" "$WORK/output" >/dev/null
grep -F "Using prebuilt runtime archives: $WORK" "$WORK/output" >/dev/null
grep -F "$WORK/perry|$WORK|1" "$PERRY_LOG" >/dev/null
[[ ! -s "$CARGO_LOG" ]]

echo "PASS"
