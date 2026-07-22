#!/usr/bin/env bash
# Run the same fast lint gates that the Tests workflow's `lint` and
# `api-docs-drift` jobs run. Designed to be invoked manually before
# `git tag vX.Y.Z` or wired into a `pre-push` git hook for branches
# that push to main / tags.
#
# This catches every gate in the Tests workflow's `lint` job — the ones
# that surprise people, because they fail on things `cargo build` never
# looks at:
#   - cargo fmt drift
#   - clippy deny-level lints ([workspace.lints] in the root Cargo.toml)
#   - the 2000-line-per-file cap (scripts/check_file_size.sh)
#   - unbarriered GC store sites / bare address-band literals
#   - the gap-snapshot checker's own self-test
#   - benchmark harness tests + public-artifact freshness
#   - untagged ```typescript fences in docs/src (Tests `doc-tests` --lint)
#   - obvious build / type errors via `cargo check`
#   - license + duplicate-dependency policy (cargo-deny), when installed
#
# What it does NOT catch (still needs full CI):
#   - doc-test runtime behavior
#   - parity vs `node --experimental-strip-types` (run
#     ./scripts/run_gap_tests.sh for that — on the .node-version pin)
#   - cross-compile builds, harmonyos smoke, etc.
#   - the changelog.d/ fragment requirement, which is PR-scoped
#
# Exit 0 = clear to tag. Non-zero = fix what's reported and re-run.
# All checks run; we print every failure before exiting, so one run
# surfaces every issue instead of trickling one per `git push`.
#
# Usage:
#   ./scripts/pre-tag-check.sh             # adds doc-fence + cargo check + clippy + deny
#   ./scripts/pre-tag-check.sh --quick     # ~10s — every no-compile gate in the lint job
#   ./scripts/pre-tag-check.sh --thorough  # ~10min — adds doc-tests run + musl cross-check
#
# --quick is the one to run habitually: it is the whole lint job minus
# anything that compiles, so it costs seconds and catches the gates that
# most often bounce a PR.
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

# ---------------------------------------------------------------------------
# Tier 1 — every `lint`-job gate that does not compile anything. Seconds
# total, and between them they account for most avoidable red PRs. Kept
# in the same order as .github/workflows/test.yml so a drift between the
# two is obvious on sight.
# ---------------------------------------------------------------------------

# 1. cargo fmt --all -- --check
run_check "cargo fmt --all --check" cargo fmt --all -- --check

# 2. Benchmark harness tests + public-artifact freshness. The freshness
#    check fingerprints the root Cargo.toml, so editing workspace deps or
#    lints invalidates it and the artifact needs regenerating with
#    ./benchmarks/run_public_baseline.sh.
run_check "benchmark harness tests" \
    python3 -m unittest discover -s tests -p 'test_benchmark_gate.py'
run_check "public benchmark evidence freshness" \
    python3 benchmarks/ci_public_baseline_check.py

# 3. File-size cap — a long doc comment is enough to trip this.
run_check "file size limit" ./scripts/check_file_size.sh

# 4. GC store-site inventory: every raw heap-slot store must be barriered
#    or carry a justified marker.
run_check "GC store-site inventory" python3 scripts/gc_store_site_inventory.py

# 5. Handle-vs-pointer address classification.
run_check "address-classification audit" python3 scripts/addr_class_inventory.py

# 6. The gap-suite ratchet's own logic. Guarded so this script works on a
#    checkout predating the snapshot ratchet (#6755).
if [[ -f scripts/gap_snapshot.py ]]; then
    run_check "gap snapshot self-test" python3 scripts/gap_snapshot.py --self-test
fi

# ---------------------------------------------------------------------------
# Tier 2 — anything that compiles. Skipped under --quick.
# ---------------------------------------------------------------------------

# 7. docs/src linter (Tests `doc-tests` matrix --lint pass)
if [[ "$mode" != "quick" ]]; then
run_check "perry-doc-tests --lint docs/src" \
    cargo run --release --quiet -p perry-doc-tests -- --lint docs/src
fi

# 8. cargo check (catches type errors fast; Tests `cargo-test` builds
#    everything anyway). Release profile on purpose — it exercises the
#    cfg paths the shipped build takes, which the dev-profile clippy run
#    below does not.
if [[ "$mode" != "quick" ]]; then
    run_check "cargo check --release --workspace" \
        cargo check --release --workspace \
            --exclude perry-ui-ios --exclude perry-ui-tvos \
            --exclude perry-ui-watchos --exclude perry-ui-visionos \
            --exclude perry-ui-android --exclude perry-ui-windows \
            --exclude perry-ui-gtk4

    # 9. Clippy — the `clippy` CI job. Exits non-zero only on the
    #    deny-level lints declared in [workspace.lints], so warn-level
    #    output here is informational.
    run_check "cargo clippy" cargo clippy

    # 10. License + duplicate-dependency policy. Optional tool: skip
    #     cleanly rather than failing a contributor who lacks it.
    if command -v cargo-deny >/dev/null 2>&1; then
        run_check "cargo deny check" cargo deny check licenses bans sources
    else
        printf '   skip: cargo-deny not installed (cargo install cargo-deny --locked)\n'
    fi
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
