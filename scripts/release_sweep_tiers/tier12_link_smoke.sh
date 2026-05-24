#!/usr/bin/env bash
# Tier 12 — link_smoke
#
# For every target triple Perry advertises (`perry --target X`), try to
# compile + link a tiny fixture and assert a non-empty artifact appears.
# We don't run the artifact — that's the simulator/emulator/host UI tier's
# job. This tier specifically catches "we broke cross-compile + link for
# target X" regressions, which the gap suite never sees.
#
# Targets are detected on the host: missing-toolchain combinations report
# SKIP with the underlying reason, not FAIL. A perry-side regression (link
# proceeds far enough to surface a real symbol error) reports FAIL.
#
# Aggregated counts:
#   passed  = number of targets that produced a non-empty artifact
#   failed  = number of targets where perry exited non-zero AND the failure
#             looks like a perry regression (not a missing-toolchain SKIP)
#   skipped = number of targets where the toolchain isn't installed
#
# The tier passes overall iff failed == 0 (skipped is permitted by design;
# this Mac doesn't have Android NDK and that's not gate-blocking).

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 12)"
LOG="$TIER_DIR/link_smoke.log"
SUMMARY="$TIER_DIR/summary.json"
PERRY_BIN="${PERRY_BIN:-$REPO_ROOT/target/release/perry}"
FIXTURE="$REPO_ROOT/tests/release/link_smoke/fixture.ts"

if [[ ! -x "$PERRY_BIN" ]]; then
    sweep_tier_emit "$OUT" 12 "link_smoke" "FAIL" 0 \
        "perry binary not found at $PERRY_BIN — run cargo build --release -p perry first"
    exit 0
fi

start="$(date +%s)"

# Each entry: target_name|host_gate|preconditions
# host_gate: comma-separated host list this target is attempted on
# preconditions: shell test that returns 0 if toolchain available, else echo a SKIP reason
TARGETS=(
    "host|all|true"
    "ios-simulator|macos|xcrun --sdk iphonesimulator --show-sdk-path >/dev/null 2>&1 || { echo 'iphonesimulator SDK not found'; exit 1; }"
    "ios|macos|xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1 || { echo 'iphoneos SDK not found'; exit 1; }"
    "tvos-simulator|macos|xcrun --sdk appletvsimulator --show-sdk-path >/dev/null 2>&1 || { echo 'appletvsimulator SDK not found'; exit 1; }"
    "tvos|macos|xcrun --sdk appletvos --show-sdk-path >/dev/null 2>&1 || { echo 'appletvos SDK not found'; exit 1; }"
    "visionos-simulator|macos|xcrun --sdk xrsimulator --show-sdk-path >/dev/null 2>&1 || { echo 'xrsimulator SDK not found'; exit 1; }"
    "visionos|macos|xcrun --sdk xros --show-sdk-path >/dev/null 2>&1 || { echo 'xros SDK not found'; exit 1; }"
    "watchos-simulator|macos|xcrun --sdk watchsimulator --show-sdk-path >/dev/null 2>&1 || { echo 'watchsimulator SDK not found'; exit 1; }"
    "watchos|macos|xcrun --sdk watchos --show-sdk-path >/dev/null 2>&1 || { echo 'watchos SDK not found'; exit 1; }"
    "android|macos,linux|[[ -n \"\${ANDROID_HOME:-\${ANDROID_SDK_ROOT:-}}\" ]] || command -v aarch64-linux-android21-clang >/dev/null 2>&1 || { echo 'Android NDK not detected (ANDROID_HOME/SDK_ROOT/clang not on PATH)'; exit 1; }"
    "linux|linux|true"
    "windows|windows|true"
    "macos|macos|true"
)

host="$(sweep_host_detect)"
declare -i passed=0
declare -i failed=0
declare -i skipped=0
declare -a failed_targets=()
declare -a skipped_targets=()
declare -a passed_targets=()

{
    echo "tier 12 link_smoke — host=$host"
    echo "fixture: $FIXTURE"
    echo "perry:   $PERRY_BIN"
    echo
} > "$LOG"

for entry in "${TARGETS[@]}"; do
    IFS='|' read -r target gate precond <<< "$entry"
    {
        echo "=== target: $target ==="
    } >> "$LOG"

    if ! sweep_tier_should_run "$gate" "$host"; then
        echo "  SKIP (host=$host gate=$gate)" >> "$LOG"
        skipped+=1
        skipped_targets+=("$target")
        continue
    fi

    # Run preconditions in a subshell so its exit doesn't kill us.
    set +e
    precond_msg="$(bash -c "$precond" 2>&1)"
    precond_rc=$?
    set -e
    if [[ "$precond_rc" -ne 0 ]]; then
        echo "  SKIP precondition: ${precond_msg:-failed}" >> "$LOG"
        skipped+=1
        skipped_targets+=("$target")
        continue
    fi

    # Build perry compile command. "host" target uses no --target flag.
    artifact="$TIER_DIR/artifact.${target//\//_}"
    rm -f "$artifact" "$artifact".app
    set +e
    if [[ "$target" == "host" ]]; then
        compile_log="$("$PERRY_BIN" "$FIXTURE" -o "$artifact" 2>&1)"
    else
        compile_log="$("$PERRY_BIN" "$FIXTURE" --target "$target" -o "$artifact" 2>&1)"
    fi
    rc=$?
    set -e

    {
        echo "  exit=$rc"
        echo "  --- perry stderr (last 12 lines) ---"
        echo "$compile_log" | tail -12 | sed 's/^/    /'
    } >> "$LOG"

    # Did it produce an artifact?
    #   - plain executable at $artifact (host, macos targets)
    #   - <name>.app bundle at $artifact.app (older convention)
    #   - $TIER_DIR/artifact.app — perry's iOS / tvOS / visionOS / watchOS app-bundle
    #     paths IGNORE the -o suffix and write to a hardcoded `artifact.app`
    #     in the output directory. Without this branch, the Apple device /
    #     simulator targets all reported FAIL even when their link succeeded.
    if [[ "$rc" -eq 0 ]] && {
            [[ -f "$artifact" && -s "$artifact" ]] ||
            [[ -d "$artifact.app" ]] ||
            [[ -d "$TIER_DIR/artifact.app" && -n "$(ls -A "$TIER_DIR/artifact.app" 2>/dev/null)" ]] ||
            [[ -d "$artifact" && -n "$(ls -A "$artifact" 2>/dev/null)" ]]
        }; then
        echo "  PASS (artifact present)" >> "$LOG"
        passed+=1
        passed_targets+=("$target")
        # Best-effort cleanup: artifacts are large, especially .app bundles.
        # Remove BOTH the suffix-named path and the hardcoded artifact.app
        # so the next target's check doesn't accidentally see a stale bundle.
        rm -rf "$artifact" "$artifact.app" "$TIER_DIR/artifact.app"
    else
        # Classify: missing-prerequisite vs perry regression.
        #   - "Could not find libperry_runtime.a" → tier 0 (build_matrix)
        #     hasn't pre-built the per-target runtime staticlib. Not a
        #     perry regression. SKIP with hint.
        #   - "Could not find libperry_stdlib_<target>.a" → same.
        #   - "command not found"/missing system linker → toolchain hole
        #     that escaped our precondition. SKIP with louder hint.
        #   - everything else → real perry regression. FAIL.
        if echo "$compile_log" | grep -qE 'Could not find libperry_(runtime|stdlib)' 2>/dev/null; then
            echo "  SKIP (per-target runtime/stdlib not pre-built — see tier 0 hint in tier 12 log)" >> "$LOG"
            echo "  hint: cargo build --release -p perry-runtime -p perry-stdlib --target <triple>" >> "$LOG"
            skipped+=1
            skipped_targets+=("$target reason=runtime-not-prebuilt")
        elif echo "$compile_log" | grep -qE 'libperry_ui_[a-z0-9_]+\.a not found' 2>/dev/null; then
            # watchOS / Android force-link the per-target UI staticlib regardless
            # of whether the user's TS imports perry/ui — see the v0.5.707 #607
            # fix. That's a precondition (UI lib must be pre-built per target),
            # not a perry regression on link smoke.
            echo "  SKIP (per-target UI staticlib not pre-built)" >> "$LOG"
            echo "  hint: cargo build --release -p perry-ui-<platform> --target <triple>" >> "$LOG"
            skipped+=1
            skipped_targets+=("$target reason=ui-staticlib-not-prebuilt")
        elif echo "$compile_log" | grep -qE 'command not found|No such file or directory.*(clang|ld|gcc|ar)|cannot find -l|library not found for' 2>/dev/null; then
            echo "  SKIP (toolchain hole not caught by precondition)" >> "$LOG"
            echo "  hint: tighten precondition for '$target' in tier12_link_smoke.sh" >> "$LOG"
            skipped+=1
            skipped_targets+=("$target reason=toolchain-hole")
        else
            echo "  FAIL (perry regression — see compile log above)" >> "$LOG"
            failed+=1
            failed_targets+=("$target")
        fi
    fi
done

# --- #1652 / #589: node:http + Web Fetch link+run smoke (host only) ---
# Guards against perry-ext-http-server's `js_node_http_*` FFI symbols, or the
# Web Fetch Headers/Request/Response constructors, silently dropping out of
# the default link. node:http server only links on the host (perry-ext-http
# isn't cross-compiled to the mobile targets), so this is host-only. The
# fixture exits immediately (never `.listen()`s), so we compile AND run it.
NH_FIXTURE="$REPO_ROOT/tests/release/link_smoke/node_http_webfetch.ts"
if [[ -f "$NH_FIXTURE" ]]; then
    nh_artifact="$TIER_DIR/node_http_webfetch"
    rm -f "$nh_artifact"
    set +e
    nh_log="$("$PERRY_BIN" "$NH_FIXTURE" -o "$nh_artifact" 2>&1)"
    nh_rc=$?
    nh_run=""
    if [[ "$nh_rc" -eq 0 && -x "$nh_artifact" ]]; then
        nh_run="$("$nh_artifact" 2>&1)"
        nh_run_rc=$?
    else
        nh_run_rc=1
    fi
    set -e
    {
        echo
        echo "=== node:http + Web Fetch smoke (host, #1652) ==="
        echo "  fixture: $NH_FIXTURE"
        echo "  compile exit=$nh_rc  run exit=$nh_run_rc"
        echo "$nh_log" | tail -8 | sed 's/^/    compile: /'
        echo "    run: $nh_run"
    } >> "$LOG"
    if [[ "$nh_rc" -eq 0 && "$nh_run_rc" -eq 0 ]] && echo "$nh_run" | grep -q "node-http-webfetch ok"; then
        echo "  PASS (node:http + Web Fetch linked and ran)" >> "$LOG"
        passed+=1
        passed_targets+=("host-node-http-webfetch")
    else
        echo "  FAIL (node:http / Web Fetch link regression — #1652)" >> "$LOG"
        failed+=1
        failed_targets+=("host-node-http-webfetch")
    fi
    rm -f "$nh_artifact"
fi

end="$(date +%s)"
dur="$((end - start))"

{
    echo
    echo "=== link_smoke summary ==="
    echo "  passed:  $passed   ${passed_targets[@]+(${passed_targets[@]})}"
    echo "  failed:  $failed   ${failed_targets[@]+(${failed_targets[@]})}"
    echo "  skipped: $skipped  ${skipped_targets[@]+(${skipped_targets[@]})}"
} >> "$LOG"

# JSON summary in the standard shape so consistency tools can read it.
fail_csv=""; skip_csv=""; pass_csv=""
[[ ${#failed_targets[@]}  -gt 0 ]] && fail_csv="$(printf '"%s",' "${failed_targets[@]}"  | sed 's/,$//')"
[[ ${#skipped_targets[@]} -gt 0 ]] && skip_csv="$(printf '"%s",' "${skipped_targets[@]}" | sed 's/,$//')"
[[ ${#passed_targets[@]}  -gt 0 ]] && pass_csv="$(printf '"%s",' "${passed_targets[@]}"  | sed 's/,$//')"
cat > "$SUMMARY" <<EOF
{"script": "tier12_link_smoke.sh", "passed": $passed, "failed": $failed, "skipped": $skipped, "passed_targets": [${pass_csv}], "failed_targets": [${fail_csv}], "skipped_targets": [${skip_csv}]}
EOF

# Tier verdict: PASS iff no perry regressions surfaced. Skips are by design.
if [[ "$failed" -eq 0 ]]; then
    sweep_tier_emit "$OUT" 12 "link_smoke" "PASS" "$dur" \
        "$passed targets linked / $skipped skipped (toolchain) / $failed regressions"
else
    sweep_tier_emit "$OUT" 12 "link_smoke" "FAIL" "$dur" \
        "$passed linked / $failed regressions: ${failed_targets[*]}"
fi
