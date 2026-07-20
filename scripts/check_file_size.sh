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
#   - crates/perry-hir/src/ir/expr.rs — a single `pub enum Expr`
#     definition (~2,560 LOC of documented variants). A lone enum
#     cannot be split across files, and decomposing it into nested
#     sub-enums would touch every match site across the codegen +
#     walker stack (a semantic refactor, not a file split). The
#     auxiliary enums and impls were already peeled into siblings;
#     the variant list itself is irreducible.
#
set -euo pipefail

THRESHOLD="${PERRY_FILE_SIZE_THRESHOLD:-2000}"

# Allowlist (one file per line; blank lines + `#` comments OK).
ALLOWLIST=$(cat <<'EOF'
# Single `pub enum Expr` definition (~2,560 LOC of documented variants). A lone
# enum can't be split across files; decomposing it into sub-enums would be a
# semantic refactor touching every match site (codegen + walker), not a file
# split. Auxiliary enums/impls already peeled into siblings; the rest is the
# irreducible variant list.
crates/perry-hir/src/ir/expr.rs
# A single ~5,650-LOC function: `run_with_parse_cache`, the per-module `par_iter`
# codegen pipeline with ~30 captured locals threaded through one closure. The
# rest of the old 6,114-line compile.rs was split into compile/ siblings
# (bootstrap/types/run_pipeline/optimized_libs/…); this trunk is now JUST that
# one deeply-coupled function. Decomposing it means extracting a context struct
# for the ~30 locals — high-risk surgery on the compiler's hot path, deferred to
# a focused follow-up. Tracked under #1435.
crates/perry/src/commands/compile/run_pipeline.rs
# node:vm surface: the Script / Module / SourceTextModule / SyntheticModule FFI
# plus a self-contained manifest-expression mini-evaluator (`EvalEnv` + the
# `eval_*` / `normalize_literal_to_json` interpreter). These share one `EvalEnv`,
# a large `FIELD_*` field-key constant set, and ~40 private helpers, so a clean
# split needs a dedicated decomposition (a shared `node_vm/common` module for
# `EvalEnv`/`FIELD_*`/helpers + an `eval` sub-module) rather than a mechanical
# file cut — deferred to a focused follow-up. Tracked under #1435.
crates/perry-runtime/src/node_vm.rs
# Generator for-of destructuring in function bodies: this PR (#5807) added
# ~22 net lines for pattern-matching VarDecl heads in async-generator iterator
# bodies. The file is 2018 lines (18 over the gate). A structural split of the
# generator for-of lowering into a sibling module is tracked as a follow-up.
crates/perry-hir/src/lower_decl/body_stmt.rs
# Promise reaction-slot occupancy fix (#5867) added ~120 net lines of
# correctness code + rationale comments across js_promise_then /
# js_promise_attach_handlers / js_promise_finally; the file is 2008 lines
# (8 over the gate). The topical split — the #1545 value-read thunks + spec
# finally-wrapper tail (~790 lines) into a sibling module — is a mechanical
# cut deferred to a focused follow-up, same pattern as body_stmt.rs above.
crates/perry-runtime/src/promise/then.rs
# --- Representation-aware type lowering (#5466 / #5464) ---
# These files crossed the gate on the type-lowering branch (native i32/u32/f64/
# i128/StringRef reps, guarded fast/fallback splits, and the material-evidence
# artifact plumbing). Each already delegates to sibling modules where the split
# was mechanical; the remainders are the coupled cores (typed-clone decision
# tables, packed-loop versioning, evidence recording) whose decomposition is a
# focused follow-up under #1435 rather than a merge-time file cut.
crates/perry-codegen/src/codegen/mod.rs
crates/perry-codegen/src/collectors/hir_facts.rs
crates/perry-codegen/src/expr/mod.rs
crates/perry-codegen/src/stmt/loops.rs
crates/perry-runtime/src/map.rs
crates/perry-runtime/src/set.rs
crates/perry-runtime/src/typed_feedback.rs
# Typed-feedback + native-ABI evidence regression suites: table-driven test
# bodies (one fn per proof shape, 185 codegen regression tests). Length tracks
# proof-surface breadth, not complexity; splitting is a test-organization
# follow-up (part is already peeled into native_proof_regressions/*.rs).
crates/perry-codegen/tests/native_proof_regressions.rs
crates/perry-runtime/src/typed_feedback/tests.rs
# `--explain-lowering` report builder: one report schema (aggregation structs +
# JSON/pretty emitters + self-tests in one file). Splitting the emitters from
# the schema they render obscures the report contract; revisit if it grows.
crates/perry/src/commands/compile/lowering_report.rs
# --- Grew a few lines past the gate on main (2026-07); each is a coupled
# core whose split is a mechanical follow-up, allowlisted here to unblock the
# required lint gate rather than fold an unrelated refactor into an in-flight
# PR. Tracked under #1435. ---
# HIR member-access lowering: the `Expr::Member` dispatch tree (per-builtin
# static-method + property arms). 2049 lines; splitting the arm groups into
# sibling modules is a mechanical follow-up.
crates/perry-hir/src/lower/expr_member.rs
# Runtime `js_native_call_method` dispatch trunk (2002 lines): the by-name
# native method switch; the peeled arms already live in native_call_method/.
crates/perry-runtime/src/object/native_call_method.rs
# Inliner call-site rewriter (2056 lines): the single `CallInliner` pass with
# its argument/return remapping tables threaded through one walker.
crates/perry-transform/src/inline/call_inliner.rs
# Windows UI widget registry (2118 lines): the Win32 widget create/dispatch
# table; a per-widget-family split is a mechanical follow-up.
crates/perry-ui-windows/src/widgets/mod.rs
# Regex grammar/parser (2075 lines): the single recursive-descent PCRE grammar;
# #6711 (ReDoS-guard bounded-quantifier collapse) pushed it 75 lines over. A
# split of the character-class / quantifier sub-parsers is a mechanical
# follow-up.
crates/perry-runtime/src/regex/grammar.rs
# Web Streams core (2011 lines): #6476's throwing-pull resilience fix added the
# js_call_catching wiring, 11 lines over. A split of the reader/tee/BYOB
# sub-controllers into siblings (tee.rs / byob already peeled) is a mechanical
# follow-up.
crates/perry-stdlib/src/streams.rs
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
