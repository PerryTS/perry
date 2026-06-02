#!/usr/bin/env bash
# release_sweep_lib.sh — shared helpers for scripts/release_sweep.sh
#
# Sourced (not executed) by release_sweep.sh and each tier script under
# scripts/release_sweep_tiers/. Every helper writes to a per-sweep output
# directory rooted at target/release-sweep/<timestamp>/ and emits one JSON
# result file per tier so the orchestrator can aggregate without reparsing
# stdout. Format is intentionally jq-free; bare bash + sed handles it.
#
# Status values:
#   PASS              tier ran and met its acceptance criteria
#   FAIL              tier ran and at least one assertion failed
#   SKIP              tier intentionally not run (host gate or --skip)
#   NOT_IMPLEMENTED   tier stub; orchestrator may still pass overall but
#                     --gate-0.6.0 will reject this
#   ERROR             tier crashed (uncaught non-zero before reaching the
#                     emit-status path); orchestrator infers this when the
#                     result.json is missing after the tier exits

set -uo pipefail

# ---------------------------------------------------------------------------
# Host detection
# ---------------------------------------------------------------------------

# Echoes one of: macos | linux | windows | unknown
sweep_host_detect() {
    case "$(uname -s 2>/dev/null || echo unknown)" in
        Darwin)              echo "macos" ;;
        Linux)               echo "linux" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)                   echo "unknown" ;;
    esac
}

# Echoes target triple shorthand for the build host. Used in report header.
sweep_host_triple() {
    local arch
    arch="$(uname -m 2>/dev/null || echo unknown)"
    case "$(sweep_host_detect)" in
        macos)   echo "${arch}-apple-darwin" ;;
        linux)   echo "${arch}-unknown-linux-gnu" ;;
        windows) echo "${arch}-pc-windows-msvc" ;;
        *)       echo "${arch}-unknown" ;;
    esac
}

# tier_should_run <gate_csv> <host>
# Returns 0 if the tier should run on this host, 1 otherwise.
# gate_csv: "all" or comma-separated list like "macos" or "macos,linux".
sweep_tier_should_run() {
    local gate="$1"
    local host="$2"
    if [[ "$gate" == "all" ]]; then
        return 0
    fi
    local IFS=','
    local g
    for g in $gate; do
        if [[ "$g" == "$host" ]]; then
            return 0
        fi
    done
    return 1
}

# ---------------------------------------------------------------------------
# Version recording
# ---------------------------------------------------------------------------

# sweep_record_versions <output_dir>
# Writes <output_dir>/versions.txt with a stable, greppable version dump.
# Missing tools record an explicit "(not found)" line instead of being silent
# so a green sweep on a half-installed machine can't accidentally hide a tier
# that should have been gated.
sweep_record_versions() {
    local out="$1/versions.txt"
    {
        echo "# Perry release sweep — versions"
        echo "# generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo
        echo "host:    $(sweep_host_detect) ($(sweep_host_triple))"
        echo "uname:   $(uname -a 2>/dev/null || echo unknown)"
        echo
        local cargo_ver
        if cargo_ver="$(grep -m1 -E '^version[[:space:]]*=' Cargo.toml 2>/dev/null | sed -E 's/.*"([^"]+)".*/\1/')"; then
            echo "perry (Cargo.toml):  ${cargo_ver:-unknown}"
        fi
        if [[ -x target/release/perry ]]; then
            echo "perry --version:     $(target/release/perry --version 2>/dev/null || echo '(error)')"
        else
            echo "perry --version:     (target/release/perry not built)"
        fi

        sweep_record_tool_version "rustc"          "rustc --version"
        sweep_record_tool_version "cargo"          "cargo --version"
        sweep_record_tool_version "node"           "node --version"
        sweep_record_tool_version "bun"            "bun --version"
        sweep_record_tool_version "deno"           "deno --version"
        sweep_record_tool_version "cc"             "cc --version"
        sweep_record_tool_version "clang"          "clang --version"
        sweep_record_tool_version "llvm-config"    "llvm-config --version"
        sweep_record_tool_version "ld"             "ld -v"
        sweep_record_tool_version "xcode-select"   "xcode-select -p"
        sweep_record_tool_version "xcrun simctl"   "xcrun simctl --version"
        sweep_record_tool_version "adb"            "adb version"
        sweep_record_tool_version "emulator"       "emulator -version"
        sweep_record_tool_version "powershell"     "powershell -Command \$PSVersionTable.PSVersion"
        sweep_record_tool_version "docker"         "docker --version"
        sweep_record_tool_version "redis-server"   "redis-server --version"
        sweep_record_tool_version "minio"          "minio --version"
        sweep_record_tool_version "mysqld"         "mysqld --version"
    } > "$out" 2>&1
}

# sweep_record_tool_version <label> <command>
# Writes one "label: <first line of stdout>" or "label: (not found)" line.
sweep_record_tool_version() {
    local label="$1"
    local cmd="$2"
    local first
    if first="$(eval "$cmd" 2>/dev/null | head -n1)"; then
        if [[ -n "$first" ]]; then
            printf '%-22s %s\n' "$label:" "$first"
            return
        fi
    fi
    printf '%-22s %s\n' "$label:" "(not found)"
}

# ---------------------------------------------------------------------------
# Per-tier result emission
# ---------------------------------------------------------------------------

# sweep_tier_dir <output_dir> <tier_id>
# Echoes (and creates) the per-tier subdirectory.
sweep_tier_dir() {
    local out="$1"
    local id="$2"
    local d
    d="$out/$(printf '%02d' "$id")"
    mkdir -p "$d"
    echo "$d"
}

# sweep_tier_emit <output_dir> <tier_id> <name> <status> <duration_seconds> <message>
# Writes <output_dir>/<NN>/result.json. Caller is responsible for the per-tier
# log file at <output_dir>/<NN>/<name>.log.
#
# JSON layout (kept flat and quote-escaped manually so we don't depend on jq):
#   {"tier": N, "name": "...", "status": "...", "duration_s": N, "message": "..."}
sweep_tier_emit() {
    local out="$1"
    local id="$2"
    local name="$3"
    local status="$4"
    local duration="$5"
    local message="${6:-}"
    local d
    d="$(sweep_tier_dir "$out" "$id")"
    local esc_name esc_status esc_message
    esc_name="$(sweep_json_escape "$name")"
    esc_status="$(sweep_json_escape "$status")"
    esc_message="$(sweep_json_escape "$message")"
    cat > "$d/result.json" <<JSON
{"tier": $id, "name": "$esc_name", "status": "$esc_status", "duration_s": $duration, "message": "$esc_message"}
JSON
}

# sweep_json_escape <string>
# Minimal JSON string escaper: \ → \\, " → \", control chars stripped to ' '.
sweep_json_escape() {
    local s="${1//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/ }"
    s="${s//$'\r'/ }"
    s="${s//$'\t'/ }"
    printf '%s' "$s"
}

# sweep_tier_run_summary <output_dir> <tier_id> <name> <command...>
# Convenience wrapper for tiers that delegate to a patched test script
# (run_parity_tests.sh, run_thread_tests.sh, etc.). Sets
# PERRY_TEST_SUMMARY_OUT to a per-tier summary.json, runs the command with
# stdout/stderr captured to <name>.log, then reads passed/failed/skipped
# back out of the summary and emits the final tier result.
#
# Called by the wired tier scripts; the caller is responsible for the
# host-gate check (the orchestrator already SKIPs out-of-scope tiers
# before invoking the tier script at all).
sweep_tier_run_summary() {
    local out="$1"; shift
    local id="$1"; shift
    local name="$1"; shift
    local d log summary start end dur rc passed failed skipped msg
    d="$(sweep_tier_dir "$out" "$id")"
    log="$d/${name}.log"
    summary="$d/summary.json"
    start="$(date +%s)"
    PERRY_TEST_SUMMARY_OUT="$summary" "$@" > "$log" 2>&1
    rc="$?"
    end="$(date +%s)"
    dur="$((end - start))"
    if [[ -f "$summary" ]]; then
        passed="$(sed -nE 's/.*"passed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$summary" | head -n1)"
        failed="$(sed -nE 's/.*"failed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$summary" | head -n1)"
        skipped="$(sed -nE 's/.*"skipped"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$summary" | head -n1)"
        msg="${passed:-?} passed / ${failed:-?} failed"
        if [[ -n "$skipped" && "$skipped" != "0" ]]; then
            msg="$msg / $skipped skipped"
        fi
    else
        msg="no summary file written"
    fi
    if [[ "$rc" -eq 0 ]]; then
        sweep_tier_emit "$out" "$id" "$name" "PASS" "$dur" "$msg"
    else
        sweep_tier_emit "$out" "$id" "$name" "FAIL" "$dur" "$msg (exit=$rc)"
    fi
}

# sweep_tier_run <output_dir> <tier_id> <name> <gate_csv> <command...>
# Convenience wrapper: gate-check, log redirection, timing, and emit.
# The command is executed as-is; non-zero exit → status FAIL.
# Caller-defined NOT_IMPLEMENTED stubs should call sweep_tier_emit directly
# rather than going through this.
sweep_tier_run() {
    local out="$1"; shift
    local id="$1"; shift
    local name="$1"; shift
    local gate="$1"; shift
    local host
    host="$(sweep_host_detect)"
    local d log start end dur rc
    d="$(sweep_tier_dir "$out" "$id")"
    log="$d/${name}.log"
    if ! sweep_tier_should_run "$gate" "$host"; then
        sweep_tier_emit "$out" "$id" "$name" "SKIP" 0 "host=$host gate=$gate"
        return 0
    fi
    start="$(date +%s)"
    "$@" > "$log" 2>&1
    rc="$?"
    end="$(date +%s)"
    dur="$((end - start))"
    if [[ "$rc" -eq 0 ]]; then
        sweep_tier_emit "$out" "$id" "$name" "PASS" "$dur" ""
    else
        sweep_tier_emit "$out" "$id" "$name" "FAIL" "$dur" "exit=$rc see ${name}.log"
    fi
    return 0
}

# ---------------------------------------------------------------------------
# Report aggregation
# ---------------------------------------------------------------------------

# sweep_report_field <result_json> <key>
# Extracts a flat key from a result.json without jq. Assumes the writer used
# sweep_tier_emit, which produces a single-line JSON object.
sweep_report_field() {
    local f="$1"
    local k="$2"
    [[ -f "$f" ]] || { echo ""; return; }
    # Match either "key": "string" or "key": number
    sed -nE "s/.*\"$k\": *\"([^\"]*)\".*/\1/p; s/.*\"$k\": *([0-9]+).*/\1/p" "$f" | head -n1
}

# sweep_format_duration <seconds>
# Echoes "1m 23s" / "12s" / "1h 2m 3s".
sweep_format_duration() {
    local s="${1:-0}"
    [[ "$s" -lt 0 ]] && s=0
    if [[ "$s" -lt 60 ]]; then
        echo "${s}s"
    elif [[ "$s" -lt 3600 ]]; then
        echo "$((s / 60))m $((s % 60))s"
    else
        echo "$((s / 3600))h $(((s % 3600) / 60))m $((s % 60))s"
    fi
}

# sweep_render_report <output_dir>
# Writes <output_dir>/report.md by walking <output_dir>/*/result.json in
# tier-id order. Also echoes the gate-relevant counts to stdout, one per line:
#   pass=N
#   fail=N
#   skip=N
#   not_implemented=N
#   error=N
# so the orchestrator can decide gate outcomes without reparsing the report.
sweep_render_report() {
    local out="$1"
    local report="$out/report.md"
    local pass=0 fail=0 skip=0 ni=0 err=0

    {
        echo "# Perry Release Sweep"
        echo
        echo "**Generated:** $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "**Host:** $(sweep_host_detect) ($(sweep_host_triple))"
        if [[ -f Cargo.toml ]]; then
            local v
            v="$(grep -m1 -E '^version[[:space:]]*=' Cargo.toml 2>/dev/null | sed -E 's/.*"([^"]+)".*/\1/')"
            echo "**Perry version:** ${v:-unknown}"
        fi
        echo
        echo "Full version dump: \`versions.txt\`"
        echo
        echo "## Summary"
        echo
        echo "| Tier | Name | Status | Duration | Notes |"
        echo "|------|------|--------|----------|-------|"
    } > "$report"

    local d res tier name status dur msg
    # Walk tier subdirs in numeric order. Glob is sorted lexically; zero-pad
    # in sweep_tier_dir keeps that aligned with numeric order.
    for d in "$out"/[0-9][0-9]; do
        [[ -d "$d" ]] || continue
        res="$d/result.json"
        if [[ ! -f "$res" ]]; then
            # Tier exited without emitting — orchestrator infers ERROR
            tier="$(basename "$d" | sed 's/^0*//')"; tier="${tier:-0}"
            name="(unknown)"
            status="ERROR"
            dur="0"
            msg="no result.json — tier crashed before emit"
        else
            tier="$(sweep_report_field "$res" tier)"
            name="$(sweep_report_field "$res" name)"
            status="$(sweep_report_field "$res" status)"
            dur="$(sweep_report_field "$res" duration_s)"
            msg="$(sweep_report_field "$res" message)"
        fi
        case "$status" in
            PASS)             pass=$((pass + 1)) ;;
            FAIL)             fail=$((fail + 1)) ;;
            SKIP)             skip=$((skip + 1)) ;;
            NOT_IMPLEMENTED)  ni=$((ni + 1)) ;;
            *)                err=$((err + 1)) ;;
        esac
        printf '| %s | %s | %s | %s | %s |\n' \
            "${tier:-?}" \
            "${name:-?}" \
            "${status:-?}" \
            "$(sweep_format_duration "${dur:-0}")" \
            "${msg:-}" \
            >> "$report"
    done

    {
        echo
        echo "**Result:** $pass passed, $fail failed, $skip skipped, $ni not-implemented, $err errored"
        echo
        echo "## Tier logs"
        echo
        for d in "$out"/[0-9][0-9]; do
            [[ -d "$d" ]] || continue
            local id; id="$(basename "$d")"
            echo "- \`$id/\` — $(ls "$d" 2>/dev/null | grep -vE '^(result\.json)$' | tr '\n' ' ')"
        done
    } >> "$report"

    echo "pass=$pass"
    echo "fail=$fail"
    echo "skip=$skip"
    echo "not_implemented=$ni"
    echo "error=$err"
}

# sweep_check_gate <output_dir> <allow_skip_csv>
# Returns 0 if the sweep should be considered green for a 0.6.0 bump:
#   - 0 fail
#   - 0 error
#   - 0 not_implemented
#   - every SKIP tier id is in allow_skip_csv
# Otherwise returns 1 and prints the violating lines to stderr.
sweep_check_gate() {
    local out="$1"
    local allow="${2:-}"
    local rc=0
    local d res status tier
    for d in "$out"/[0-9][0-9]; do
        [[ -d "$d" ]] || continue
        res="$d/result.json"
        if [[ ! -f "$res" ]]; then
            echo "GATE FAIL: tier $(basename "$d") produced no result.json (ERROR)" >&2
            rc=1
            continue
        fi
        status="$(sweep_report_field "$res" status)"
        tier="$(sweep_report_field "$res" tier)"
        case "$status" in
            PASS) ;;
            SKIP)
                local IFS=','
                local found=0 a
                for a in $allow; do
                    if [[ "$a" == "$tier" ]]; then
                        found=1; break
                    fi
                done
                if [[ "$found" -eq 0 ]]; then
                    echo "GATE FAIL: tier $tier ($status) not in --allow-skip" >&2
                    rc=1
                fi
                ;;
            *)
                echo "GATE FAIL: tier $tier ($status)" >&2
                rc=1
                ;;
        esac
    done
    return "$rc"
}
