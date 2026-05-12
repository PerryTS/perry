#!/usr/bin/env bash
# Run the same fast lint gates that the Tests workflow's `lint` and
# `api-docs-drift` jobs run. Designed to be invoked manually before
# `git tag vX.Y.Z` or wired into a `pre-push` git hook for branches
# that push to main / tags.
#
# This catches:
#   - cargo fmt drift (Tests workflow `lint` job)
#   - untagged ```typescript fences in docs/src (Tests `doc-tests` job's
#     --lint pass)
#   - obvious cargo build / type errors via `cargo check` (Tests
#     `cargo-test` + `compile-smoke` builds)
#
# What it does NOT catch (still needs full CI):
#   - doc-test runtime behavior
#   - parity vs `node --experimental-strip-types`
#   - cross-compile builds, harmonyos smoke, etc.
#
# Exit 0 = clear to tag. Non-zero = fix what's reported and re-run.
# All checks run; we print every failure before exiting, so one run
# surfaces every issue instead of trickling one per `git push`.
#
# Usage:
#   ./scripts/pre-tag-check.sh             # ~30s — fmt + doc-fence + cargo check
#   ./scripts/pre-tag-check.sh --quick     # ~5s  — fmt + doc-fence only
#   ./scripts/pre-tag-check.sh --thorough  # ~10min — adds doc-tests run + musl cross-check
#
# --thorough is recommended before tagging if you suspect Perry-side
# behavior may have shifted (HIR / codegen / state-desugar changes).
# It catches every Mac-reproducible class of failure we hit on CI:
# real Perry bugs (the .value state desugar trio that ate two tag
# cycles), HIR routing gaps (WebView 1-arg), api-manifest gaps
# (ethers.Wallet), and musl-specific cfg gates (RTLD_DEEPBIND).

set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

mode="default"
case "${1:-}" in
    --quick) mode="quick" ;;
    --thorough) mode="thorough" ;;
    "") mode="default" ;;
    *)
        printf 'unknown flag: %s\nusage: %s [--quick|--thorough]\n' "$1" "$0" >&2
        exit 2
        ;;
esac

failures=()

step() {
    printf '\n\033[1;36m==>\033[0m %s\n' "$1"
}

run_check() {
    local label="$1"; shift
    step "$label"
    if "$@"; then
        printf '\033[1;32m   ok\033[0m: %s\n' "$label"
    else
        printf '\033[1;31m   FAIL\033[0m: %s\n' "$label"
        failures+=("$label")
    fi
}

# 1. cargo fmt --all -- --check (Tests `lint` job)
run_check "cargo fmt --all --check" cargo fmt --all -- --check

# 2. docs/src linter (Tests `doc-tests` matrix --lint pass)
run_check "perry-doc-tests --lint docs/src" \
    cargo run --release --quiet -p perry-doc-tests -- --lint docs/src

# 3. cargo check (catches type errors fast; Tests `cargo-test` builds
#    everything anyway). Skipped under --quick.
if [[ "$mode" != "quick" ]]; then
    run_check "cargo check --release --workspace" \
        cargo check --release --workspace \
            --exclude perry-ui-ios --exclude perry-ui-tvos \
            --exclude perry-ui-watchos --exclude perry-ui-visionos \
            --exclude perry-ui-android --exclude perry-ui-windows \
            --exclude perry-ui-gtk4
fi

# --thorough adds two more passes that catch Linux/musl-specific
# regressions and runtime Perry behavior that fmt + cargo check can't
# see (HIR rewrites, codegen routing, api-manifest gating).
if [[ "$mode" == "thorough" ]]; then
    # 4. Run the macOS doc-test suite end-to-end with the same
    #    --filter-exclude shape the Tests workflow uses. Catches
    #    real Perry bugs (state desugar, WebView 1-arg routing,
    #    api-manifest class lookup, etc.).
    run_check "perry-doc-tests run (--skip-xcompile, excl gallery)" \
        cargo run --release --quiet -p perry-doc-tests -- \
            --skip-xcompile --filter-exclude ui/gallery.ts

    # 5. cargo check against musl. Catches `cfg(target_os = "linux")`
    #    gates that should be `cfg(all(target_os = "linux", target_env = "gnu"))`
    #    (e.g. RTLD_DEEPBIND, glibc-only libc constants). Only runs
    #    if the musl target is installed — `rustup target add
    #    x86_64-unknown-linux-musl` to enable.
    if rustup target list --installed 2>/dev/null | grep -q "^x86_64-unknown-linux-musl$"; then
        run_check "cargo check --target x86_64-unknown-linux-musl -p perry-runtime" \
            cargo check --release --target x86_64-unknown-linux-musl \
                -p perry-runtime -p perry-stdlib
    else
        printf '   skip: x86_64-unknown-linux-musl target not installed (rustup target add x86_64-unknown-linux-musl)\n'
    fi
fi

echo
if [[ ${#failures[@]} -eq 0 ]]; then
    printf '\033[1;32mAll pre-tag checks passed.\033[0m Safe to tag.\n'
    exit 0
fi

printf '\033[1;31mPre-tag checks FAILED:\033[0m\n'
for f in "${failures[@]}"; do
    printf '  - %s\n' "$f"
done
exit 1
