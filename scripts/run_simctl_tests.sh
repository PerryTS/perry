#!/usr/bin/env bash
# Run UI doc-examples inside an Apple Simulator (iOS / tvOS / visionOS /
# watchOS) via xcrun simctl. Originally iOS-only; PLATFORM env var
# generalizes to the other Apple sims for release_sweep.sh tiers 8 and 9.
#
# Requires Xcode + the relevant SDK. Each UI example that lists
# `${PLATFORM}-simulator` in its banner `targets:` line gets compiled with
# `perry compile --target ${PLATFORM}-simulator`, installed on the booted
# simulator, and launched with `PERRY_UI_TEST_MODE=1
# PERRY_UI_TEST_EXIT_AFTER_MS=500`. perry-ui-<platform>'s
# install_test_mode_exit_timer() exits(0) after the first frame; a timeout
# wrapper kills the process if the exit never fires.
#
# Usage:
#   DEVICE="iPhone 15" ./scripts/run_simctl_tests.sh                    # iOS (default)
#   PLATFORM=tvos ./scripts/run_simctl_tests.sh                         # tvOS
#   PLATFORM=visionos ./scripts/run_simctl_tests.sh                     # visionOS
#   PLATFORM=watchos ./scripts/run_simctl_tests.sh                      # watchOS
#   ./scripts/run_simctl_tests.sh --filter ui/counter                   # subset
#
# Env:
#   PLATFORM            — ios | tvos | visionos | watchos (default: ios)
#   DEVICE              — simulator device name (default: per-platform; see below)
#   PERRY_BIN           — path to perry (default: target/release/perry)
#   BUNDLE_ID_PREFIX    — bundle-id prefix (default: com.perry.doctests)
#   KEEP_BOOTED         — if "1", don't shut the simulator down after run
#   LAUNCH_TIMEOUT      — per-example launch timeout in seconds (default: 30)
#   PERRY_TEST_SUMMARY_OUT — release_sweep.sh hook (see comment in run_parity_tests.sh)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PLATFORM="${PLATFORM:-ios}"
case "$PLATFORM" in
    ios)      DEVICE_DEFAULT="iPhone 15" ; SIM_FILTER='iPhone|iPad' ;;
    tvos)     DEVICE_DEFAULT="Apple TV 4K (3rd generation)" ; SIM_FILTER='Apple TV' ;;
    visionos) DEVICE_DEFAULT="Apple Vision Pro" ; SIM_FILTER='Vision' ;;
    watchos)  DEVICE_DEFAULT="Apple Watch Ultra 2 (49mm)" ; SIM_FILTER='Apple Watch' ;;
    *) echo "unknown PLATFORM=$PLATFORM (expected ios|tvos|visionos|watchos)" >&2; exit 2 ;;
esac
SIM_TARGET="${PLATFORM}-simulator"

DEVICE="${DEVICE:-$DEVICE_DEFAULT}"
PERRY_BIN="${PERRY_BIN:-$REPO_ROOT/target/release/perry}"
BUNDLE_ID_PREFIX="${BUNDLE_ID_PREFIX:-com.perry.doctests}"
LAUNCH_TIMEOUT="${LAUNCH_TIMEOUT:-30}"

# Export so the inline `python3 -c '...'` UDID lookup below sees DEVICE.
# Without this export, callers that set DEVICE via assignment-then-call
# (rather than `DEVICE=... ./script`) leak through to a python subprocess
# with DEVICE unset and a KeyError aborts the whole script.
export DEVICE

# macOS doesn't ship GNU `timeout`. Homebrew's coreutils provides it as
# `gtimeout`. Fall back to a pure-bash `&+sleep+kill` watchdog if neither
# is on PATH — slower startup, same guarantee.
if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_CMD="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_CMD="gtimeout"
else
    TIMEOUT_CMD=""
fi

run_with_timeout() {
    local secs=$1; shift
    if [ -n "$TIMEOUT_CMD" ]; then
        "$TIMEOUT_CMD" "$secs" "$@"
    else
        # Fallback: fork, sleep-kill watchdog. Preserves the child's
        # exit code via `wait`.
        "$@" &
        local pid=$!
        ( sleep "$secs" && kill -TERM -- "$pid" 2>/dev/null ) &
        local watcher=$!
        if wait "$pid" 2>/dev/null; then
            kill -TERM -- "$watcher" 2>/dev/null
            wait "$watcher" 2>/dev/null
            return 0
        else
            local rc=$?
            kill -TERM -- "$watcher" 2>/dev/null
            wait "$watcher" 2>/dev/null
            # Bash conventionally returns 124 for timeouts; emulate that
            # when our child was killed after the deadline expired.
            if [ "$rc" = "143" ]; then return 124; fi
            return "$rc"
        fi
    fi
}

FILTER=""
while [ $# -gt 0 ]; do
    case "$1" in
        --filter) FILTER="$2"; shift 2 ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

if ! command -v xcrun >/dev/null 2>&1; then
    echo "xcrun not found — install Xcode / command-line tools" >&2
    exit 1
fi
if [ ! -x "$PERRY_BIN" ]; then
    echo "perry binary not found at $PERRY_BIN (run \`cargo build --release -p perry\` first)" >&2
    exit 1
fi

OUT_DIR="$REPO_ROOT/target/perry-simctl-tests"
mkdir -p "$OUT_DIR"

# Find or boot the target device.
UDID=$(xcrun simctl list devices available -j \
    | python3 -c "
import json, sys, os
target = os.environ['DEVICE']
data = json.load(sys.stdin)
for rt, devs in data['devices'].items():
    for d in devs:
        if d['name'] == target and d['isAvailable']:
            print(d['udid'])
            sys.exit(0)
")

if [ -z "$UDID" ]; then
    echo "No available simulator device named '$DEVICE' for platform $PLATFORM. Try:" >&2
    xcrun simctl list devices available | grep -E "$SIM_FILTER" | head -10
    exit 1
fi

echo "Using $PLATFORM device $DEVICE ($UDID)"

# Boot if not already.
STATE=$(xcrun simctl list devices | grep "$UDID" | grep -oE "\((Booted|Shutdown)\)" || true)
if [ "$STATE" != "(Booted)" ]; then
    echo "Booting $DEVICE ..."
    xcrun simctl boot "$UDID"
fi
xcrun simctl bootstatus "$UDID" -b

# Iterate UI examples whose banner includes ios-simulator.
TOTAL=0; PASS=0; FAIL=0
FAILURES=()

while IFS= read -r -d '' src; do
    rel="${src#$REPO_ROOT/}"
    if [ -n "$FILTER" ] && [[ "$rel" != *"$FILTER"* ]]; then continue; fi
    # Require `<platform>-simulator` in the targets banner.
    if ! head -15 "$src" | grep -qE "^// *targets:.*${SIM_TARGET}"; then continue; fi

    TOTAL=$((TOTAL+1))
    stem=$(basename "${src%.ts}")
    bin_out="$OUT_DIR/${stem}"
    app_dir="${bin_out}.app"
    bundle_id="${BUNDLE_ID_PREFIX}.${stem}"

    rm -rf "$app_dir"

    echo "=== $rel ==="
    echo "  [+] compile start $(date +%T)"
    if ! "$PERRY_BIN" compile --target "$SIM_TARGET" --app-bundle-id "$bundle_id" "$src" -o "$bin_out" >"$OUT_DIR/$stem.compile.log" 2>&1; then
        echo "  COMPILE_FAIL (see $OUT_DIR/$stem.compile.log)"
        FAIL=$((FAIL+1)); FAILURES+=("$rel COMPILE_FAIL"); continue
    fi
    echo "  [+] compile done $(date +%T)"
    if [ ! -d "$app_dir" ]; then
        echo "  NO_BUNDLE ($app_dir missing after compile)"
        FAIL=$((FAIL+1)); FAILURES+=("$rel NO_BUNDLE"); continue
    fi

    echo "  [+] install start $(date +%T)"
    if ! xcrun simctl install "$UDID" "$app_dir" >/dev/null 2>&1; then
        echo "  INSTALL_FAIL"
        FAIL=$((FAIL+1)); FAILURES+=("$rel INSTALL_FAIL"); continue
    fi
    echo "  [+] install done $(date +%T)"

    # Launch with PERRY_UI_TEST_MODE so the app self-exits after one frame.
    # simctl launch has NO --setenv flag. Env vars reach the spawned app by
    # prefixing them with SIMCTL_CHILD_ in the calling shell's environment
    # (see `xcrun simctl help launch` — the SIMCTL_CHILD_ note at the end).
    # Prior attempts to pass --setenv=KEY=VALUE or --setenv KEY=VALUE both
    # failed with "Invalid device: --setenv..." because simctl parsed the
    # unknown flag as the positional device argument.
    #
    # Deliberately NO --console-pty: it blocks simctl until the app's stdio
    # closes, but when simctl's own stdout is redirected to a file (as here)
    # the PTY half never reports EOF to simctl even after the app calls
    # process::exit(0). Result: simctl hangs past our LAUNCH_TIMEOUT and the
    # whole step stalls without printing PASS before GitHub kills the runner.
    # Without --console-pty, simctl returns immediately with the child pid;
    # the launch exit code still catches bundle/arch/signing errors (it's
    # how we surfaced the v0.5.149 Info.plist mismatch), and the test-mode
    # exit timer ensures the app doesn't linger on the simulator between
    # examples. We trade "did the app cleanly exit?" for "did the bundle
    # at least launch?" — still a strong tier-2 signal.
    echo "  [+] launch start $(date +%T)"
    # No run_with_timeout wrapper: without --console-pty simctl returns
    # in <1s; the bash watchdog fallback in run_with_timeout races its
    # own `kill -TERM <watcher>` against the watcher's own exit on the
    # fast path, and set -e turns the losing race into a script abort
    # (observed as exit 143 at ~2s post-launch in run #24759762125).
    set +e
    SIMCTL_CHILD_PERRY_UI_TEST_MODE=1 \
    SIMCTL_CHILD_PERRY_UI_TEST_EXIT_AFTER_MS=500 \
    xcrun simctl launch \
        --terminate-running-process \
        "$UDID" "$bundle_id" >"$OUT_DIR/$stem.run.log" 2>&1
    rc=$?
    set -e
    echo "  [+] launch done $(date +%T) rc=$rc"
    if [ "$rc" -ne 0 ]; then
        if [ "$rc" = "124" ]; then
            echo "  TIMEOUT (> ${LAUNCH_TIMEOUT}s)"
            FAIL=$((FAIL+1)); FAILURES+=("$rel TIMEOUT")
        else
            echo "  RUN_FAIL (exit $rc, see $OUT_DIR/$stem.run.log)"
            FAIL=$((FAIL+1)); FAILURES+=("$rel RUN_FAIL")
        fi
        # Best-effort cleanup so the next iteration starts clean.
        xcrun simctl terminate "$UDID" "$bundle_id" >/dev/null 2>&1 || true
        continue
    fi

    echo "  PASS"
    PASS=$((PASS+1))
    echo "  [+] uninstall start $(date +%T)"
    xcrun simctl uninstall "$UDID" "$bundle_id" >/dev/null 2>&1 || true
    echo "  [+] uninstall done $(date +%T)"
done < <(find "$REPO_ROOT/docs/examples" -name "*.ts" -print0)

echo
echo "simctl-tests: $PASS/$TOTAL passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    printf '%s\n' "${FAILURES[@]}"
fi

# release_sweep.sh hook — see comment in run_parity_tests.sh.
if [ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]; then
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_simctl_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0, "total": $TOTAL, "platform": "$PLATFORM"}
EOF
fi

if [ "${KEEP_BOOTED:-0}" != "1" ]; then
    xcrun simctl shutdown "$UDID" || true
fi

[ "$FAIL" -eq 0 ]
