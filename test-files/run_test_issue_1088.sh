#!/bin/bash
# Wire-level regression harness for issue #1088 — `--output-type staticlib`
# + unified Event Loop FFI for host embedding.
# See test_issue_1088_staticlib.ts / test_issue_1088_host.c for the writeup.
#
# Usage:
#   PERRY_BIN=./target/release/perry ./test-files/run_test_issue_1088.sh
#
# Assertions:
#   * `--output-type staticlib` produces an `ar` archive whose only entry
#     point is `perry_module_init` (no `T main` symbol — would collide).
#   * A C host program linked against the archive + libperry_runtime.a +
#     libperry_stdlib.a can call `perry_module_init`, `perry_poll`,
#     `perry_has_work`, `perry_next_wake_ms`, and `perry_set_wake_callback`.
#   * After registering a wake callback, `js_notify_main_thread()` from the
#     same thread fires it.
#
# macOS-only for now — Linux works in principle (just swap the framework
# args) but the parity / compile-smoke CI lanes are tag-gated and the
# host-embedding lane isn't wired up yet (#1088 explicitly asked for the
# surface first; CI plumbing is the follow-up). The script bails cleanly
# on other platforms so it doesn't fail the suite there.

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "[1088] SKIP — host-embedding smoke is macOS-only for now"
    exit 0
fi

PERRY_BIN="${PERRY_BIN:-./target/release/perry}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TS_SRC="$SCRIPT_DIR/test_issue_1088_staticlib.ts"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/perry-1088.XXXXXX")"
STATIC_LIB="$WORK_DIR/libperry_tslib.a"
HOST_EXE="$WORK_DIR/smoke"
C_SRC="$WORK_DIR/host.c"
cat > "$C_SRC" <<'CHOST'
/* Host smoke for #1088 — embedded here rather than a sibling .c because
 * test-files/test_* is gitignored unless it matches *.ts/*.tsx. The body
 * exercises perry_module_init, the unified Event Loop facade, and the
 * wake-callback registration round-trip. */
#include <stdio.h>
#include <stdlib.h>
extern void   perry_module_init(void);
extern int    perry_poll(void);
extern int    perry_has_work(void);
extern double perry_next_wake_ms(void);
extern void   js_notify_main_thread(void);
extern void   perry_set_wake_callback(void (*cb)(void *), void *ctx);
static int wake_fired = 0;
static void on_wake(void *ctx) { (void)ctx; wake_fired = 1; }
int main(void) {
    printf("[host] calling perry_module_init\n");
    perry_module_init();
    printf("[host] perry_module_init returned\n");
    int polled = perry_poll();
    int has = perry_has_work();
    double next = perry_next_wake_ms();
    printf("[host] perry_poll=%d perry_has_work=%d perry_next_wake_ms=%.1f\n",
           polled, has, next);
    perry_set_wake_callback(on_wake, NULL);
    js_notify_main_thread();
    printf("[host] wake_fired=%d\n", wake_fired);
    perry_set_wake_callback(NULL, NULL);
    if (wake_fired != 1) {
        fprintf(stderr, "[host] FAIL — wake callback did not fire after notify\n");
        return 2;
    }
    if (next != -1.0) {
        fprintf(stderr, "[host] FAIL — idle program reported a pending wake (next=%.1f)\n", next);
        return 3;
    }
    return 0;
}
CHOST

cd "$WORKSPACE_ROOT"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL: perry binary not found at $PERRY_BIN — build first via cargo build --release -p perry" >&2
    exit 1
fi

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

echo "[1088] compiling staticlib..."
PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY_BIN" "$TS_SRC" -o "$STATIC_LIB" \
    --output-type staticlib >/dev/null

# The runtime + stdlib archives that the codegen auto-optimized; the host has
# to bring these along at its own link step. Pick the most recent
# perry-auto-* dir — `perry compile` keeps the path opaque, so we scan.
AUTO_DIR="$(ls -td "$WORKSPACE_ROOT"/target/perry-auto-*/release 2>/dev/null | head -n 1)"
if [[ -z "$AUTO_DIR" ]]; then
    AUTO_DIR="$WORKSPACE_ROOT/target/release"
fi
RUNTIME_LIB="$AUTO_DIR/libperry_runtime.a"
STDLIB_LIB="$AUTO_DIR/libperry_stdlib.a"

for lib in "$STATIC_LIB" "$RUNTIME_LIB" "$STDLIB_LIB"; do
    if [[ ! -f "$lib" ]]; then
        echo "[1088] FAIL — missing $lib" >&2
        exit 1
    fi
done

echo "[1088] checking archive contents..."
if nm "$STATIC_LIB" 2>&1 | grep -qE '^[0-9a-f]+ T _?main$'; then
    echo "[1088] FAIL — archive exports 'main', will collide with host" >&2
    exit 1
fi
if ! nm "$STATIC_LIB" 2>&1 | grep -qE '^[0-9a-f]+ T _?perry_module_init$'; then
    echo "[1088] FAIL — archive missing 'perry_module_init' entry point" >&2
    exit 1
fi

# Linkdeps sidecar: must exist next to the archive and list at least the
# runtime archive — that's the one a host can't link without (everything
# else is optional, but runtime is always required by `perry_module_init`).
MANIFEST="${STATIC_LIB%.a}.linkdeps.json"
if [[ ! -f "$MANIFEST" ]]; then
    echo "[1088] FAIL — linkdeps sidecar not written at $MANIFEST" >&2
    exit 1
fi
if ! grep -q '"role": "runtime"' "$MANIFEST"; then
    echo "[1088] FAIL — linkdeps sidecar missing runtime archive entry" >&2
    cat "$MANIFEST" >&2
    exit 1
fi
if ! grep -q '"entry_symbol": "perry_module_init"' "$MANIFEST"; then
    echo "[1088] FAIL — linkdeps sidecar missing entry_symbol" >&2
    cat "$MANIFEST" >&2
    exit 1
fi

echo "[1088] linking host smoke..."
cc -o "$HOST_EXE" "$C_SRC" \
    "$STATIC_LIB" "$RUNTIME_LIB" "$STDLIB_LIB" \
    -framework Foundation \
    -framework CoreFoundation \
    -framework Security \
    -framework SystemConfiguration \
    -lc++ \
    >/dev/null 2>&1

echo "[1088] running host..."
OUTPUT="$("$HOST_EXE" 2>&1)"
echo "$OUTPUT" | sed 's/^/    /'

fail=0
if [[ "$OUTPUT" != *"[ts] module init: hello from staticlib"* ]]; then
    echo "[1088] FAIL — perry_module_init didn't actually run the TS module body"
    fail=1
fi
if [[ "$OUTPUT" != *"perry_poll=0 perry_has_work=0 perry_next_wake_ms=-1.0"* ]]; then
    echo "[1088] FAIL — idle facade defaults regressed"
    fail=1
fi
if [[ "$OUTPUT" != *"wake_fired=1"* ]]; then
    echo "[1088] FAIL — perry_set_wake_callback didn't fire on notify"
    fail=1
fi

if [[ $fail -eq 0 ]]; then
    echo "[1088] PASS"
    exit 0
else
    exit 1
fi
