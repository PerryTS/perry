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
# Object field get/set + handle/native dispatch shim; grew past the limit
# after the #1419 KeyObject/.export/.equals routing + main's process-module
# additions. Splitting tracked under #1435.
crates/perry-runtime/src/object/field_get_set.rs
# Codegen `Call` dispatch tower; grew past the limit after #1419's crypto
# fast-path gate refinements + main's process / fs / perf_hooks Expr
# additions. Splitting per-builtin family tracked alongside #1435.
crates/perry-codegen/src/expr/calls.rs
# Dynamic method-call dispatch tower (js_native_call_method); crossed the
# limit by the 7-line WeakMap/WeakSet dispatch hook in #1757/#1758. Splitting
# per receiver-kind family tracked alongside #1435.
crates/perry-runtime/src/object/native_call_method.rs
# Codegen driver: `compile_module` is a single ~2,100-LOC function (module
# setup -> per-class field-layout -> per-fn codegen -> link). It crossed the
# limit after #26's cross-module same-named-class disambiguation (the
# class_field_counts / class_init_chains build, interleaved with the per-class
# loop). Extracting that pass is high-risk surgery deferred to the codegen
# split tracked under #1435.
crates/perry-codegen/src/codegen/mod.rs
# node:stream runtime (Readable/Writable/Duplex + EventEmitter listener
# lifecycle). Crossed the limit during the stream-parity PR wave (#1962/
# #1963/#1976/#1981 …); splitting mid-wave would conflict with several open
# stream PRs that all touch this file. Split tracked under #1987.
crates/perry-runtime/src/node_stream.rs
# Stream parity test surface; co-evolves with the node_stream.rs module and
# crossed the limit after #2198 added the readable-read-size cases. Splitting
# would scatter assertions away from the inputs they exercise — defer until
# the node_stream.rs split under #1987 lands, then re-cluster tests under the
# new sub-modules.
crates/perry-runtime/src/node_stream_tests.rs
# Central class registry — class IDs, prototypes, parent-closure
# scanning, and field-init replay. Crossed the limit after the
# #1787 instance-field init replay (#2074) + web-stream class
# wiring (#1641/#2110). Split tracked under #1435.
crates/perry-runtime/src/object/class_registry.rs
# Native-module namespace property/method dispatcher
# (`get_native_module_constant` is one big match — one arm per
# stdlib namespace, every property literal inline). Splitting per
# namespace would scatter arms that share helpers (`fs_const`,
# `os_signal_const`, …) and the constants tables they index.
# Crossed the limit at 2014 LOC after the #2135 worker_threads
# value-export arm. Split tracked under #1435.
crates/perry-runtime/src/object/native_module.rs
# console.log / util.inspect value formatter (two big per-tag dispatch
# towers: format_jsvalue + format_jsvalue_for_json). main had already
# grown it to 1999 LOC; the #2089 Date-as-reference-type inspect arms
# (a DateCell pointer must render as its ISO string, not be deref'd as an
# ObjectHeader) tipped it over. Splitting the two towers into sibling
# modules is tracked under #1435.
crates/perry-runtime/src/builtins/formatting.rs
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
