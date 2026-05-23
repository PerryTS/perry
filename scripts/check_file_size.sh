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
#   - crates/perry-runtime/src/gc/tests.rs — left behind by the gc.rs
#     split in the #1090 GC architecture checkpoint. The companion
#     production files in `gc/` all came in under 2k; only the test
#     fixture remained big. Re-evaluate once the GC owner peels it
#     apart.
#   - crates/perry-codegen-arkts/src/tests.rs — ArkTS golden-output
#     test fixtures. Top-down test scaffolding, not production code;
#     splitting would split assertions away from the inputs that
#     produced them.
#   - crates/perry-api-manifest/src/entries.rs — generated-feel
#     manifest table (one entry per public API surface item). Length
#     reflects API breadth, not complexity, and splitting would scatter
#     entries that ought to live next to each other for drift review.
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
crates/perry-runtime/src/gc/tests.rs
crates/perry-codegen-arkts/src/tests.rs
crates/perry-api-manifest/src/entries.rs
crates/perry/src/commands/compile.rs
# Native-module dispatch table; one big match by (module, method, class).
# Splitting per-namespace is tracked under the API-manifest refactor in #793.
crates/perry-codegen/src/lower_call/native/mod.rs
# HIR `Expr` enum + dependency-walker arms; splitting would need parallel
# updates across every variant of the walker traits. Tracked alongside #793.
crates/perry-hir/src/ir/expr.rs
# node:crypto stdlib surface expanded by #1419 (sign/verify, RSA, EC, DH,
# AES-GCM, …). Splitting into per-algorithm sub-modules is tracked as a
# follow-up under #793.
crates/perry-stdlib/src/crypto.rs
# WebCrypto subtle.* surface expanded by #1419. Split-by-algorithm tracked
# as a follow-up under #793.
crates/perry-stdlib/src/webcrypto.rs
# Object field get/set + handle/native dispatch shim; grew past the limit
# after the #1419 KeyObject/.export/.equals routing + main's process-module
# additions. Splitting tracked under #1435.
crates/perry-runtime/src/object/field_get_set.rs
# Codegen `Call` dispatch tower; grew past the limit after #1419's crypto
# fast-path gate refinements + main's process / fs / perf_hooks Expr
# additions. Splitting per-builtin family tracked alongside #1435.
crates/perry-codegen/src/expr/calls.rs
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
