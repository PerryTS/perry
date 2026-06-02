#!/usr/bin/env bash
# Run UI doc-examples on an Android emulator via adb.
#
# This is the Android counterpart to scripts/run_simctl_tests.sh. Each UI
# example whose banner targets line includes `android` gets compiled with
# `perry compile --target android`, installed via `adb install`, launched
# with the PERRY_UI_TEST_MODE intent extra, and observed via adb logcat
# for the perry-ui-android exit-after-first-frame signal.
#
# Required: ANDROID_HOME (or ANDROID_SDK_ROOT), the `emulator` binary, an
# AVD configured (any), `adb`. Tier 10 of release_sweep.sh detects missing
# preconditions BEFORE running this script — but this script also checks
# them so it can be invoked standalone.
#
# Env:
#   ANDROID_AVD_NAME   — AVD to boot (default: first AVD listed by avdmanager)
#   PERRY_BIN          — path to perry (default: target/release/perry)
#   BOOT_TIMEOUT       — seconds to wait for boot complete (default: 180)
#   LAUNCH_TIMEOUT     — seconds per example (default: 60)
#   PERRY_TEST_SUMMARY_OUT — release_sweep.sh hook
#   KEEP_BOOTED        — if "1", don't shut down the emulator after run

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PERRY_BIN="${PERRY_BIN:-$REPO_ROOT/target/release/perry}"
BOOT_TIMEOUT="${BOOT_TIMEOUT:-180}"
LAUNCH_TIMEOUT="${LAUNCH_TIMEOUT:-60}"

# Resolve SDK
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$SDK" ]]; then
    echo "android-emu: ANDROID_HOME / ANDROID_SDK_ROOT not set" >&2
    exit 2
fi

EMULATOR_BIN="$SDK/emulator/emulator"
[[ -x "$EMULATOR_BIN" ]] || EMULATOR_BIN="$(command -v emulator 2>/dev/null || true)"
ADB_BIN="$SDK/platform-tools/adb"
[[ -x "$ADB_BIN" ]] || ADB_BIN="$(command -v adb 2>/dev/null || true)"
AVDMANAGER_BIN="$SDK/cmdline-tools/latest/bin/avdmanager"
[[ -x "$AVDMANAGER_BIN" ]] || AVDMANAGER_BIN="$(command -v avdmanager 2>/dev/null || true)"

if [[ ! -x "$EMULATOR_BIN" ]] || [[ ! -x "$ADB_BIN" ]] || [[ ! -x "$AVDMANAGER_BIN" ]]; then
    echo "android-emu: required binary missing" >&2
    echo "  emulator:    ${EMULATOR_BIN:-(not found)}" >&2
    echo "  adb:         ${ADB_BIN:-(not found)}" >&2
    echo "  avdmanager:  ${AVDMANAGER_BIN:-(not found)}" >&2
    exit 2
fi

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "android-emu: perry binary not found at $PERRY_BIN" >&2
    exit 2
fi

# Pick an AVD to boot
AVD="${ANDROID_AVD_NAME:-}"
if [[ -z "$AVD" ]]; then
    AVD="$("$AVDMANAGER_BIN" list avd | sed -nE 's/^[[:space:]]*Name:[[:space:]]+(.*)$/\1/p' | head -1)"
fi
if [[ -z "$AVD" ]]; then
    echo "android-emu: no AVD configured. Create one with avdmanager / Android Studio." >&2
    exit 2
fi

OUT_DIR="$REPO_ROOT/target/perry-android-tests"
mkdir -p "$OUT_DIR"

echo "android-emu: AVD=$AVD"

# Boot emulator in background
"$EMULATOR_BIN" -avd "$AVD" -no-snapshot -no-audio -no-window -gpu swiftshader_indirect \
    > "$OUT_DIR/emulator.log" 2>&1 &
EMU_PID=$!

cleanup() {
    if [[ "${KEEP_BOOTED:-0}" != "1" ]]; then
        echo "android-emu: shutting down emulator..."
        "$ADB_BIN" emu kill >/dev/null 2>&1 || true
        kill "$EMU_PID" 2>/dev/null || true
        wait "$EMU_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Wait for boot complete
echo "android-emu: waiting for boot complete (timeout ${BOOT_TIMEOUT}s)..."
deadline=$(( $(date +%s) + BOOT_TIMEOUT ))
while [[ $(date +%s) -lt $deadline ]]; do
    if "$ADB_BIN" shell getprop sys.boot_completed 2>/dev/null | grep -q 1; then
        echo "android-emu: boot complete"
        break
    fi
    sleep 2
done
if ! "$ADB_BIN" shell getprop sys.boot_completed 2>/dev/null | grep -q 1; then
    echo "android-emu: boot did not complete within ${BOOT_TIMEOUT}s" >&2
    exit 1
fi

# Iterate UI examples whose banner includes android
TOTAL=0
PASS=0
FAIL=0
FAILURES=()

while IFS= read -r -d '' src; do
    rel="${src#$REPO_ROOT/}"
    if ! head -15 "$src" | grep -qE "^// *targets:.*android"; then continue; fi

    TOTAL=$((TOTAL+1))
    stem="$(basename "${src%.ts}")"
    apk="$OUT_DIR/${stem}.apk"
    pkg_id="com.perry.doctests.${stem}"

    echo "=== $rel ==="
    echo "  [+] perry compile --target android"
    if ! "$PERRY_BIN" compile --target android --app-bundle-id "$pkg_id" "$src" -o "$apk" \
            > "$OUT_DIR/$stem.compile.log" 2>&1; then
        echo "  COMPILE_FAIL"
        FAIL=$((FAIL+1)); FAILURES+=("$rel COMPILE_FAIL")
        continue
    fi
    if [[ ! -s "$apk" ]]; then
        echo "  NO_APK"
        FAIL=$((FAIL+1)); FAILURES+=("$rel NO_APK")
        continue
    fi

    echo "  [+] adb install"
    if ! "$ADB_BIN" install -r "$apk" > "$OUT_DIR/$stem.install.log" 2>&1; then
        echo "  INSTALL_FAIL"
        FAIL=$((FAIL+1)); FAILURES+=("$rel INSTALL_FAIL")
        continue
    fi

    echo "  [+] adb shell am start (PERRY_UI_TEST_MODE)"
    "$ADB_BIN" logcat -c >/dev/null 2>&1 || true
    if ! "$ADB_BIN" shell am start \
            --es PERRY_UI_TEST_MODE 1 \
            --ei PERRY_UI_TEST_EXIT_AFTER_MS 500 \
            -n "${pkg_id}/.PerryActivity" \
            > "$OUT_DIR/$stem.run.log" 2>&1; then
        echo "  LAUNCH_FAIL"
        FAIL=$((FAIL+1)); FAILURES+=("$rel LAUNCH_FAIL")
        "$ADB_BIN" uninstall "$pkg_id" >/dev/null 2>&1 || true
        continue
    fi

    # Watch logcat for clean exit signal or crash
    deadline=$(( $(date +%s) + LAUNCH_TIMEOUT ))
    saw_exit=0
    while [[ $(date +%s) -lt $deadline ]]; do
        if "$ADB_BIN" logcat -d -s PerryUI:I 2>/dev/null | grep -qE "test-mode exit"; then
            saw_exit=1
            break
        fi
        if "$ADB_BIN" logcat -d 2>/dev/null | grep -qE "FATAL EXCEPTION|FORCE_FINISHING|Process: $pkg_id" | grep -qE "FATAL|FORCE"; then
            break
        fi
        sleep 1
    done

    "$ADB_BIN" uninstall "$pkg_id" >/dev/null 2>&1 || true

    if [[ "$saw_exit" -eq 1 ]]; then
        echo "  PASS"
        PASS=$((PASS+1))
    else
        echo "  TIMEOUT_OR_CRASH"
        FAIL=$((FAIL+1)); FAILURES+=("$rel TIMEOUT_OR_CRASH")
    fi
done < <(find "$REPO_ROOT/docs/examples" -name "*.ts" -print0)

echo
echo "android-emu: $PASS/$TOTAL passed, $FAIL failed"
[[ $FAIL -gt 0 ]] && printf '  %s\n' "${FAILURES[@]+${FAILURES[@]}}"

if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_android_emu_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0, "total": $TOTAL, "platform": "android", "avd": "$AVD"}
EOF
fi

[[ $FAIL -eq 0 ]]
