#!/usr/bin/env bash
#
# CI gate: fail if any tracked Rust source file exceeds the LOC threshold.
#
# Big single-file modules are hard to read, hard to review, and hurt
# build incrementality (touching one symbol invalidates the IDE +
# cargo-check work for thousands of lines downstream). This script
# enforces an upper bound and is run on every PR.
#
# Threshold is **2,000 lines** as of v0.5.1020. Started at 5,000 in
# v0.5.1019 with the first wave of splits (compile.rs / expr/mod.rs /
# native_table.rs / etc.), tightened to 2,000 once the long-tail
# 2k-5k files were split topically (lower_decl/, inline/, json/,
# stable_hash/, builtins/, array/, monomorph/, publish/, arena/,
# emit/, generator/, js_transform/, modules/, run/, promise/, setup/,
# string/, ir/, runtime_decls/, value/, perry-ui-{macos,ios,android,
# visionos,tvos,windows,gtk4}/, closure/, walker/, dispatch/, lower/,
# buffer/, destructuring/, lower_call/native/, interop/, stmt/, url/,
# bridge/, deforest/, compile/link/, compile/cjs_wrap/, …).
#
# Scope: only checks `*.rs` files. Other formats (JS runtime
# templates, HTML examples, Kotlin templates, JSON fixtures, dist
# bundles) intentionally not policed — they aren't really "review
# surface" the way production Rust is.
#
# Allowlisted (real Rust source, deferred for a specific reason —
# **each entry needs a one-line rationale**):
#
#   - crates/perry/src/commands/compile.rs — the deeply-coupled
#     `par_iter` codegen closure inside `run_with_parse_cache`
#     (~1,800 LOC, ~30 captured locals) needs extraction into a
#     context-struct helper. High-risk surgery deferred to a
#     follow-up PR; the rest of compile.rs was already split into
#     compile/{types,bootstrap,bundle_apple,...} sub-modules
#     (16 siblings in compile/).
#
set -euo pipefail

THRESHOLD="${PERRY_FILE_SIZE_THRESHOLD:-2000}"

# Allowlist (one file per line; blank lines + `#` comments OK).
ALLOWLIST=$(cat <<'EOF'
crates/perry/src/commands/compile.rs
# HIR `Expr` enum + dependency-walker arms; splitting would need parallel
# updates across every variant of the walker traits. Tracked alongside #793.
crates/perry-hir/src/ir/expr.rs
# stdlib native dispatch table; current main crossed the threshold after
# namespace-alias exposure work. Split tracked under #1435.
crates/perry-stdlib/src/common/dispatch.rs
# SQLite stdlib shim remains a generated-feel native adapter table; current
# main crossed the threshold before this PR. Split tracked under #1435.
crates/perry-stdlib/src/sqlite.rs
# Shared stdlib dispatch bridge crossed the gate on current main; split per
# dispatch family with the stdlib dispatch cleanup tracked in #1435.
crates/perry-stdlib/src/common/dispatch.rs
# sqlite stdlib remains a monolithic binding surface on current main; split
# statements/sessions/backups/functions in the sqlite cleanup tracked in #1435.
crates/perry-stdlib/src/sqlite.rs
# perry-stdlib container module root — re-exports `perry_container_compose::*`
# and the `js_container_*` / `js_compose_*` FFI dispatch surface (gated behind
# the `container` feature). Splitting the FFI surface per command family is
# tracked under #1435.
crates/perry-stdlib/src/container/mod.rs
# node:events bundled module (EventEmitter handle surface, once/on helpers,
# AbortSignal wiring, AsyncResource). Crossed the 2000-line gate after the
# `events.on(...)` real async-iterator rewrite (proper { next, return } over a
# buffered/pending-promise queue, replacing the bare-array stub). Splitting the
# on/once iterator machinery into the existing `events/` submodule is tracked
# under #1435 with the other module-size cleanups.
crates/perry-stdlib/src/events.rs
# auto-optimize libs driver. Crossed the 2000-line gate after the
# fresh-archive-reuse work (#4928) added the build-stamp + freshness probe
# (`auto_optimized_archives_are_fresh` / `auto_optimized_build_stamp` /
# `auto_optimized_cache_key`) and their regression tests next to the existing
# `build_optimized_libs` driver + well-known resolution. Splitting the
# freshness/well-known helpers into a sibling module is tracked under #1435.
crates/perry/src/commands/compile/optimized_libs.rs
EOF
)

# Anchor at repo root so the script can be invoked from anywhere.
cd "$(git rev-parse --show-toplevel)"

# Build the offender list — tracked Rust files only.
violations=""
total=0
while IFS= read -r f; do
    [ -f "$f" ] || continue

    # Allowlist match.
    if grep -Fxq "$f" <<<"$ALLOWLIST"; then continue; fi

    lines=$(wc -l < "$f" 2>/dev/null || echo 0)
    if [ "$lines" -gt "$THRESHOLD" ]; then
        violations+="$(printf '%7d  %s\n' "$lines" "$f")"$'\n'
        total=$((total + 1))
    fi
done < <(git ls-files '*.rs')

if [ "$total" -gt 0 ]; then
    echo "::error::File size limit exceeded ($THRESHOLD lines)."
    echo ""
    echo "The following files are too large:"
    echo "$violations"
    echo ""
    echo "Split the offending files into topical sub-modules. See"
    echo "v0.5.1019/v0.5.1020 commits on chore/split-large-files for"
    echo "the recipe: extract function groups into sibling files,"
    echo "re-export from mod.rs with explicit named use statements"
    echo "(globs don't propagate through transitive re-exports). To"
    echo "deliberately exclude a file (e.g. a refactor in progress"
    echo "tracked elsewhere) add it to the ALLOWLIST block at the top"
    echo "of this script with a one-line rationale."
    exit 1
fi

echo "OK: no Rust source files exceed $THRESHOLD lines."
