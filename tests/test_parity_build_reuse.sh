#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cp "$ROOT/run_parity_tests.sh" "$WORK/run_parity_tests.sh"
mkdir -p "$WORK/scripts"
cp "$ROOT/scripts/run_gap_tests.sh" "$WORK/scripts/run_gap_tests.sh"
cp "$ROOT/scripts/gap_snapshot.py" "$WORK/scripts/gap_snapshot.py"
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
touch "$WORK/test-files/test_gap_reuse.ts"
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

# Exercise the Git Bash branch without requiring a Windows host. The mock
# compiler receives and creates an `.exe`, `.lib` archives satisfy the
# prebuilt-artifact check, and TEMP supplies the run-scoped scratch root when
# TMPDIR is absent.
cp "$WORK/perry" "$WORK/perry.exe"
chmod +x "$WORK/perry.exe"
touch "$WORK/perry_runtime.lib" "$WORK/perry_stdlib.lib"
mkdir -p "$WORK/windows-temp"
: > "$PERRY_LOG"
set +e
env -u TMPDIR -u TMP \
    PERRY_HOST_PLATFORM=windows \
    TEMP="$WORK/windows-temp" \
    PERRY_SKIP_BUILD=1 \
    PERRY_BIN="$WORK/perry.exe" \
    PERRY_RUNTIME_DIR="$WORK" \
    "$WORK/run_parity_tests.sh" --suite node-suite --module reuse >"$WORK/windows-output" 2>&1
status=$?
set -e
[[ "$status" -eq 0 ]]
grep -F "Using prebuilt compiler: $WORK/perry.exe" "$WORK/windows-output" >/dev/null
grep -F "$WORK/perry.exe|$WORK|1" "$PERRY_LOG" >/dev/null
grep -F '"platform": "windows"' "$WORK/test-parity/reports/latest.json" >/dev/null
if find "$WORK/windows-temp" -maxdepth 1 -name 'perry-parity.*' | grep -q .; then
    echo "Windows scratch directory was not cleaned" >&2
    exit 1
fi

# The gap wrapper must select an independent Windows snapshot instead of
# comparing the Windows result with the committed Linux baseline.
env -u TMPDIR -u TMP \
    PERRY_HOST_PLATFORM=windows \
    TEMP="$WORK/windows-temp" \
    PERRY_SKIP_BUILD=1 \
    PERRY_BIN="$WORK/perry.exe" \
    PERRY_RUNTIME_DIR="$WORK" \
    "$WORK/scripts/run_gap_tests.sh" --filter test_gap_reuse >"$WORK/gap-output" 2>&1
grep -F "test-parity/gap_snapshot.windows.json" "$WORK/gap-output" >/dev/null

echo "PASS"
