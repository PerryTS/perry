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
if [ "${PERRY_TEST_HANG:-0}" = "1" ]; then
    sleep 30
fi
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

fail() { echo "ASSERTION FAILED: $*" >&2; exit 1; }

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
[[ "$status" -eq 0 ]] || fail "expected a clean run, got exit $status"
grep -F "Using prebuilt compiler: $WORK/perry" "$WORK/output" >/dev/null
grep -F "Using prebuilt runtime archives: $WORK" "$WORK/output" >/dev/null
grep -F "$WORK/perry|$WORK|1" "$PERRY_LOG" >/dev/null
[[ ! -s "$CARGO_LOG" ]] || fail "cargo was invoked despite PERRY_SKIP_BUILD=1"

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
[[ "$status" -eq 0 ]] || fail "expected a clean run, got exit $status"
grep -F "Using prebuilt compiler: $WORK/perry.exe" "$WORK/windows-output" >/dev/null
grep -F "$WORK/perry.exe|$WORK|1" "$PERRY_LOG" >/dev/null
grep -F '"platform": "windows"' "$WORK/test-parity/reports/latest.json" >/dev/null
if find "$WORK/windows-temp" -maxdepth 1 -name 'perry-parity.*' | grep -q .; then
    echo "Windows scratch directory was not cleaned" >&2
    exit 1
fi

# Git Bash does not ship GNU timeout. Exercise the Python-backed Windows
# process-tree timeout with a one-second test hook and require crash
# classification rather than waiting for the mock binary's 30-second sleep.
SECONDS=0
set +e
env -u TMPDIR -u TMP \
    PERRY_HOST_PLATFORM=windows \
    PERRY_RUN_TIMEOUT=1 \
    PERRY_TEST_HANG=1 \
    TEMP="$WORK/windows-temp" \
    PERRY_SKIP_BUILD=1 \
    PERRY_BIN="$WORK/perry.exe" \
    PERRY_RUNTIME_DIR="$WORK" \
    "$WORK/run_parity_tests.sh" --suite node-suite --module reuse >"$WORK/timeout-output" 2>&1
timeout_status=$?
set -e
[[ "$timeout_status" -ne 0 ]] || fail "expected non-zero exit from the timeout path"
grep -F "TIMEOUT (killed after 1s)" "$WORK/timeout-output" >/dev/null
if (( SECONDS > 8 )); then
    echo "Windows timeout fallback took ${SECONDS}s" >&2
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
    "$WORK/scripts/run_gap_tests.sh" --filter test_gap_reuse >"$WORK/gap-output" 2>&1 || {
        gap_status=$?
        echo "gap wrapper failed:" >&2
        cat "$WORK/gap-output" >&2
        exit "$gap_status"
    }
grep -F "test-parity/gap_snapshot.windows.json" "$WORK/gap-output" >/dev/null

# ── Streaming journal + resume ──────────────────────────────────────────────
# The runner appends each completed test to a JSONL journal as it finishes, and
# that journal is both the progress stream's source and the resume checkpoint.
# Covered here: results are journaled during the run, --resume skips what is
# already recorded and reproduces an identical report, a torn final line costs
# at most the one test that was in flight, and a different compiler/runtime
# build is refused instead of being blended into one report.
mkdir -p "$WORK/test-parity/node-suite/resume"
for i in 1 2 3 4; do
    touch "$WORK/test-parity/node-suite/resume/case$i.ts"
done
JOURNAL="$WORK/test-parity/reports/journal/parity_node-suite_mod-resume.jsonl"
journal_run() {
    PERRY_SKIP_BUILD=1 PERRY_BIN="$WORK/perry" "$WORK/run_parity_tests.sh" \
        --suite node-suite --module resume "$@"
}
mask_generated() { sed 's/"generated_at": "[^"]*"/"generated_at": "MASKED"/' "$1"; }
count_progress() { grep -cE '^\[[0-9]+/4\]' "$1" || true; }
truncate_journal() { head -3 "$JOURNAL" > "$JOURNAL.part"; mv "$JOURNAL.part" "$JOURNAL"; }

# A plain run journals every test and streams exactly one progress line each.
journal_run >"$WORK/j1.out" 2>"$WORK/j1.err"
[[ -f "$JOURNAL" ]] || fail "journal was not created at $JOURNAL"
[[ "$(grep -c '"kind":"header"' "$JOURNAL")" -eq 1 ]] || fail "journal header missing"
[[ "$(grep -c '"kind":"result"' "$JOURNAL")" -eq 4 ]] || fail "expected 4 journaled results"
[[ "$(count_progress "$WORK/j1.err")" -eq 4 ]] || fail "expected 4 streamed progress lines, got $(count_progress "$WORK/j1.err")"
cp "$WORK/test-parity/reports/latest.json" "$WORK/j1.json"

# Resuming a complete journal re-runs nothing and reports the same results.
journal_run --resume >"$WORK/j2.out" 2>"$WORK/j2.err"
grep -F "4 from journal" "$WORK/j2.out" >/dev/null
[[ "$(count_progress "$WORK/j2.err")" -eq 0 ]] || fail "a fully-journaled resume re-ran tests"
diff <(mask_generated "$WORK/j1.json") \
     <(mask_generated "$WORK/test-parity/reports/latest.json") >/dev/null \
    || fail "resumed report differs from the uninterrupted report"

# A partial journal (header + 2 results) resumes and lands on the same report.
truncate_journal
journal_run --resume >"$WORK/j3.out" 2>"$WORK/j3.err"
grep -F "2 from journal" "$WORK/j3.out" >/dev/null
[[ "$(count_progress "$WORK/j3.err")" -eq 2 ]] || fail "partial resume ran $(count_progress "$WORK/j3.err") tests, expected 2"
diff <(mask_generated "$WORK/j1.json") \
     <(mask_generated "$WORK/test-parity/reports/latest.json") >/dev/null \
    || fail "resumed report differs from the uninterrupted report"

# A torn final line is what a kill -9 mid-write leaves behind. It must be
# sealed before appending: otherwise the next record concatenates onto it and
# BOTH become unparseable, silently losing a second, innocent test.
truncate_journal
printf '{"kind":"result","id":"node-suite/resume/case' >> "$JOURNAL"
journal_run --resume >"$WORK/j4.out" 2>"$WORK/j4.err"
grep -F "unterminated final line" "$WORK/j4.out" >/dev/null
[[ "$(count_progress "$WORK/j4.err")" -eq 2 ]] || fail "torn-line resume ran $(count_progress "$WORK/j4.err") tests, expected 2"
diff <(mask_generated "$WORK/j1.json") \
     <(mask_generated "$WORK/test-parity/reports/latest.json") >/dev/null \
    || fail "resumed report differs from the uninterrupted report"

# Resuming across a different runtime archive must hard-fail. The perry binary
# is untouched here: a stale libperry_*.a changes behavior on its own, so the
# fingerprint has to cover the archives too.
truncate_journal
printf 'changed\n' > "$WORK/libperry_runtime.a"
run_failure "Refusing to resume" env PERRY_SKIP_BUILD=1 PERRY_BIN="$WORK/perry" \
    "$WORK/run_parity_tests.sh" --suite node-suite --module resume --resume
: > "$WORK/libperry_runtime.a"

echo "PASS"
