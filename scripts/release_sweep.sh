#!/usr/bin/env bash
# release_sweep.sh — orchestrate a full pre-release test sweep.
#
# Runs every tier in scripts/release_sweep_tiers/ in numeric order, gating
# each one on the host OS, and writes a single aggregated report to
# target/release-sweep/<timestamp>/report.md.
#
# Each tier is a separate script that emits one result.json (via the
# sweep_tier_emit helper in release_sweep_lib.sh). The orchestrator never
# needs to parse a tier's stdout — it only reads result.json — which means
# tiers can be implemented in any language as long as they call the lib's
# emitter at the end.
#
# Usage:
#   scripts/release_sweep.sh                              # run everything for the host
#   scripts/release_sweep.sh --tier=3                     # just tier 3
#   scripts/release_sweep.sh --tier=3,7,12                # subset
#   scripts/release_sweep.sh --skip=10                    # everything except tier 10
#   scripts/release_sweep.sh --quick                      # short-circuit env: each tier
#                                                          # may shorten its workload
#   scripts/release_sweep.sh --gate-0.6.0                 # exit non-zero unless every
#                                                          # tier passes (no SKIP without
#                                                          # --allow-skip, no NOT_IMPLEMENTED)
#   scripts/release_sweep.sh --gate-0.6.0 --allow-skip=11 # green even if tier 11 SKIPs
#                                                          # (e.g. running on macOS)
#
# Output:
#   target/release-sweep/<YYYYMMDD-HHMMSS>/
#     report.md
#     versions.txt
#     <NN>/result.json     # one per tier
#     <NN>/<name>.log      # tier's raw output
#
# Tier registry below — single source of truth. Adding a new tier means:
#   1. Drop tierNN_<name>.sh in scripts/release_sweep_tiers/
#   2. Add a row here with: id|name|gate|description
# `gate` is "all" or comma-separated host list (macos | linux | windows).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TIERS_DIR="$SCRIPT_DIR/release_sweep_tiers"
cd "$REPO_ROOT"

# shellcheck source=release_sweep_lib.sh
. "$SCRIPT_DIR/release_sweep_lib.sh"

# Tier registry. Keep IDs zero-padded and unique.
TIER_REGISTRY=(
    "00|build_matrix|all|cargo build for every shipped target triple"
    "01|cargo_workspace|all|cargo test --workspace with host-appropriate exclusions"
    "02|parity|all|run_parity_tests.sh — gap + edge suites byte-vs-Node"
    "03|real_packages|all|drizzle/hono/s3-lite/mysql/redis/fastify/ws/axios fixtures"
    "04|gc_stress|all|run_memory_stability_tests.sh × {gen,evac,wb,classic}"
    "05|threading|all|run_thread_tests.sh"
    "06|doc_tests|all|run_doc_tests.sh / .ps1"
    "07|ui_host_smoke|all|run_ui_styling_matrix.sh + headless host launch"
    "08|sim_apple|macos|iOS / tvOS / visionOS simulator (run_simctl_tests.sh)"
    "09|sim_watchos|macos|watchOS simulator"
    "10|android_emu|macos,linux|Android emulator via avdmanager + adb"
    "11|windows_smoke|windows|Native Windows app smoke (smoke_windows_app.ps1)"
    "12|link_smoke|all|cross-compile + link a tiny App per target triple"
    "13|native_abi_evidence|all|native ABI evidence packet smoke gate"
)

# ---------------------------------------------------------------------------
# Flag parsing
# ---------------------------------------------------------------------------

opt_only_tiers=""
opt_skip_tiers=""
opt_quick=0
opt_gate=0
opt_allow_skip=""
opt_output=""

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Options:
  --tier=<id[,id...]>     run only the listed tiers (zero-padded or bare ids)
  --skip=<id[,id...]>     run everything except the listed tiers
  --quick                 set PERRY_RELEASE_SWEEP_QUICK=1 for tier scripts
  --gate-0.6.0            exit non-zero unless every applicable tier passed
  --allow-skip=<id[,...]> tiers permitted to SKIP under --gate-0.6.0
  --output=<dir>          override target/release-sweep/<timestamp>
  -h, --help              show this message

Tier registry:
EOF
    local row id name gate desc
    for row in "${TIER_REGISTRY[@]}"; do
        IFS='|' read -r id name gate desc <<< "$row"
        printf '  %2s  %-18s  [%s]  %s\n' "$id" "$name" "$gate" "$desc"
    done
}

for arg in "$@"; do
    case "$arg" in
        --tier=*)        opt_only_tiers="${arg#--tier=}" ;;
        --skip=*)        opt_skip_tiers="${arg#--skip=}" ;;
        --quick)         opt_quick=1 ;;
        --gate-0.6.0)    opt_gate=1 ;;
        --allow-skip=*)  opt_allow_skip="${arg#--allow-skip=}" ;;
        --output=*)      opt_output="${arg#--output=}" ;;
        -h|--help)       usage; exit 0 ;;
        *)               echo "unknown flag: $arg" >&2; usage; exit 2 ;;
    esac
done

# Normalize id lists ("3" → "03", "3,11" → "03 11")
sweep_normalize_ids() {
    local raw="$1"
    [[ -z "$raw" ]] && return 0
    local IFS=','
    local v
    for v in $raw; do
        printf '%02d ' "$((10#$v))"
    done
}
norm_only="$(sweep_normalize_ids "$opt_only_tiers")"
norm_skip="$(sweep_normalize_ids "$opt_skip_tiers")"

sweep_id_in_list() {
    local needle="$1"
    local haystack="$2"
    [[ -z "$haystack" ]] && return 1
    case " $haystack " in
        *" $needle "*) return 0 ;;
    esac
    return 1
}

# ---------------------------------------------------------------------------
# Output dir
# ---------------------------------------------------------------------------

if [[ -n "$opt_output" ]]; then
    output_dir="$opt_output"
else
    ts="$(date +%Y%m%d-%H%M%S)"
    output_dir="$REPO_ROOT/target/release-sweep/$ts"
fi
mkdir -p "$output_dir"

echo "release_sweep: output → $output_dir"
sweep_record_versions "$output_dir"

# Export shared env every tier can read
export PERRY_RELEASE_SWEEP_OUTPUT="$output_dir"
export PERRY_RELEASE_SWEEP_QUICK="$opt_quick"
export PERRY_RELEASE_SWEEP_HOST="$(sweep_host_detect)"

# ---------------------------------------------------------------------------
# Tier loop
# ---------------------------------------------------------------------------

host="$(sweep_host_detect)"
ran=0
for row in "${TIER_REGISTRY[@]}"; do
    IFS='|' read -r id name gate desc <<< "$row"
    if [[ -n "$norm_only" ]] && ! sweep_id_in_list "$id" "$norm_only"; then
        continue
    fi
    if sweep_id_in_list "$id" "$norm_skip"; then
        sweep_tier_emit "$output_dir" "$((10#$id))" "$name" "SKIP" 0 "skipped via --skip"
        continue
    fi
    if ! sweep_tier_should_run "$gate" "$host"; then
        sweep_tier_emit "$output_dir" "$((10#$id))" "$name" "SKIP" 0 "host=$host gate=$gate"
        continue
    fi
    tier_script="$TIERS_DIR/tier${id}_${name}.sh"
    if [[ ! -x "$tier_script" ]]; then
        sweep_tier_emit "$output_dir" "$((10#$id))" "$name" "ERROR" 0 "tier script missing or not executable: $tier_script"
        continue
    fi
    echo "release_sweep: tier $id ($name) starting..."
    start="$(date +%s)"
    # Tier script is responsible for emitting its own result.json. Capture
    # crash-shaped exits (non-zero with no result.json) by checking after.
    if ! "$tier_script"; then
        end="$(date +%s)"
        if [[ ! -f "$output_dir/$id/result.json" ]]; then
            sweep_tier_emit "$output_dir" "$((10#$id))" "$name" "ERROR" "$((end - start))" "tier exited non-zero without emitting result.json"
        fi
    fi
    ran=$((ran + 1))
done

# ---------------------------------------------------------------------------
# Aggregate
# ---------------------------------------------------------------------------

counts="$(sweep_render_report "$output_dir")"
echo "release_sweep: ran $ran tiers"
echo "$counts" | sed 's/^/release_sweep: /'
echo "release_sweep: report → $output_dir/report.md"

if [[ "$opt_gate" -eq 1 ]]; then
    norm_allow="$(sweep_normalize_ids "$opt_allow_skip" | sed 's/  */,/g; s/,$//')"
    if sweep_check_gate "$output_dir" "$norm_allow"; then
        echo "release_sweep: --gate-0.6.0 → GREEN"
        echo "release_sweep: suggested bump → [workspace.package] version = \"0.6.0\""
        exit 0
    else
        echo "release_sweep: --gate-0.6.0 → RED" >&2
        exit 1
    fi
fi

exit 0
