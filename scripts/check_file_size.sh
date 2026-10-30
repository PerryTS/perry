#!/usr/bin/env bash
#
# CI gate: fail if any tracked Rust source file exceeds the LOC threshold.
#
# Big single-file modules are hard to read, hard to review, and hurt
# build incrementality (touching one symbol invalidates the IDE +
# cargo-check work for thousands of lines downstream). This script
# enforces an upper bound and is run on every PR.
#
# What counts is CODE. Comments, doc comments, blank lines and block-comment
# continuation lines do not count: a reviewer reads code, and pricing a
# comment like a line of code pushes an author to delete the reasoning instead
# of splitting the file. Test code still counts, because it is review surface
# too. Measured this way the tree carries ~15% fewer lines than `wc -l` sees,
# which is why ten allowlist entries were retired when the rule changed.
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
# Allowlisted (real Rust source, deferred for a specific reason):
#
# Each entry is `<path><TAB><code-line count>`. The count is asserted, not
# documentary: the gate fails when the file's current size stops matching it,
# so an exemption has to be re-argued instead of silently covering growth. An
# entry justified at "2009 lines" once guarded 5,430. `--update` re-records
# every count and drops the entries that no longer need one; an entry naming a
# path that is not tracked fails the gate on its own.
#
# Every entry also carries a one-line rationale:
#
#   - crates/perry-hir/src/ir/expr.rs — a single `pub enum Expr`
#     definition (~2,560 LOC of documented variants). A lone enum
#     cannot be split across files, and decomposing it into nested
#     sub-enums would touch every match site across the codegen +
#     walker stack (a semantic refactor, not a file split). The
#     auxiliary enums and impls were already peeled into siblings;
#     the variant list itself is irreducible.
#
# Vendored third-party sources are also allowlisted (currently
# third_party/windows-winui/*): they are an upstream snapshot, not
# Perry review surface, and must stay byte-identical so the vendor
# drop can be re-diffed. Each still carries its own one-line
# rationale in the ALLOWLIST block below.
#
set -euo pipefail

THRESHOLD="${PERRY_FILE_SIZE_THRESHOLD:-2000}"

# Allowlist (one file per line as `<path><TAB><count>`; blank lines + `#`
# comments OK; `--update` rewrites it).
ALLOWLIST=$(cat <<'EOF'
# A single ~5,650-LOC function: `run_with_parse_cache`, the per-module `par_iter`
# codegen pipeline with ~30 captured locals threaded through one closure. The
# rest of the old 6,114-line compile.rs was split into compile/ siblings
# (bootstrap/types/run_pipeline/optimized_libs/…); this trunk is now JUST that
# one deeply-coupled function. Decomposing it means extracting a context struct
# for the ~30 locals — high-risk surgery on the compiler's hot path, deferred to
# a focused follow-up. Tracked under #1435.
crates/perry/src/commands/compile/run_pipeline.rs	6182
# Generator for-of destructuring in function bodies: this PR (#5807) added
# ~22 net lines for pattern-matching VarDecl heads in async-generator iterator
# bodies. The file is 2018 lines (18 over the gate). A structural split of the
# generator for-of lowering into a sibling module is tracked as a follow-up.
crates/perry-hir/src/lower_decl/body_stmt.rs	2063
# --- Representation-aware type lowering (#5466 / #5464) ---
# These files crossed the gate on the type-lowering branch (native i32/u32/f64/
# i128/StringRef reps, guarded fast/fallback splits, and the material-evidence
# artifact plumbing). Each already delegates to sibling modules where the split
# was mechanical; the remainders are the coupled cores (typed-clone decision
# tables, packed-loop versioning, evidence recording) whose decomposition is a
# focused follow-up under #1435 rather than a merge-time file cut.
crates/perry-codegen/src/codegen/mod.rs	3574
crates/perry-codegen/src/collectors/hir_facts.rs	2740
crates/perry-codegen/src/expr/mod.rs	2798
crates/perry-codegen/src/stmt/loops.rs	7677
crates/perry-runtime/src/map.rs	2845
crates/perry-runtime/src/set.rs	2335
crates/perry-runtime/src/typed_feedback.rs	2437
# Typed-feedback + native-ABI evidence regression suites: table-driven test
# bodies (one fn per proof shape, 185 codegen regression tests). Length tracks
# proof-surface breadth, not complexity; splitting is a test-organization
# follow-up (part is already peeled into native_proof_regressions/*.rs).
crates/perry-codegen/tests/native_proof_regressions.rs	15627
crates/perry-runtime/src/typed_feedback/tests.rs	2278
# `--explain-lowering` report builder: one report schema (aggregation structs +
# JSON/pretty emitters + self-tests in one file). Splitting the emitters from
# the schema they render obscures the report contract; revisit if it grows.
crates/perry/src/commands/compile/lowering_report.rs	2320
# Inliner call-site rewriter (2056 lines): the single `CallInliner` pass with
# its argument/return remapping tables threaded through one walker.
crates/perry-transform/src/inline/call_inliner.rs	2091
# Web Streams core (2011 lines): #6476's throwing-pull resilience fix added the
# js_call_catching wiring, 11 lines over. A split of the reader/tee/BYOB
# sub-controllers into siblings (tee.rs / byob already peeled) is a mechanical
# follow-up.
crates/perry-stdlib/src/streams.rs	2418
# GC policy core (2009 lines): phase-C descriptor-summary accounting pushed the
# coupled collection-policy state machine nine lines over the gate on main. Its
# phase/debt/budget groups should be split together in the tracked #1435 file
# decomposition rather than mixed into an unrelated runtime fast-path PR.
crates/perry-runtime/src/gc/policy.rs	2765
# The shape-registry core is 16 lines over after documenting mutable private
# tombstone epochs. Its table/index/publish invariants need a coordinated split
# rather than moving one half of the invariant for this optimization.
crates/perry-runtime/src/object/shapes.rs	4782
# Exact qualified Turnloop snapshot; retain upstream layout for comparison.
# Source hashes and registry provenance: third_party/turnloop/VENDORED.md.
third_party/turnloop/turnloop/src/driver.rs	2283
# Machine-generated WinRT metadata bindings for Windows.Foundation.Collections.
third_party/windows-winui/windows-collections/src/bindings.rs	3032
# Machine-generated WinRT metadata bindings for Microsoft.UI.Xaml — the whole
# WinUI 3 control surface in one generated file (26k lines).
third_party/windows-winui/windows-reactor/src/bindings.rs	25479
# Upstream Windows Reactor XAML backend: the single Element -> Microsoft.UI.Xaml
# realization/diff trunk shipped as one module by upstream.
third_party/windows-winui/windows-reactor/src/winui/backend/mod.rs	4032
EOF
)

# Anchor at repo root so the script can be invoked from anywhere.
cd "$(git rev-parse --show-toplevel)"

# What counts as code. A reviewer reads code; comments and blank lines are not
# code, and pricing them like code pushes an author to delete the reasoning
# rather than split the file. Test code still counts — it is review surface too.
code_lines() {
    awk '
        { line = $0; sub(/^[ \t]+/, "", line) }
        line == ""      { next }
        line ~ /^\/\//  { next }
        line ~ /^\/\*/  { next }
        line ~ /^\*/    { next }
        { n++ }
        END { printf "%d", n + 0 }
    ' "$1"
}

# The decision, isolated from the repository so --self-test can exercise every
# branch: verdict <code-lines> <asserted-or-empty> <threshold>
verdict() {
    if [ -n "$2" ]; then
        if [ "$1" = "$2" ]; then echo exempt; else echo stale; fi
        return
    fi
    if [ "$1" -gt "$3" ]; then echo over; else echo within; fi
}

asserted_for() {
    printf '%s\n' "$ALLOWLIST" | awk -F'\t' -v p="$1" '
        $0 ~ /^[[:space:]]*#/ { next }
        NF >= 2 && $1 == p   { print $2; exit }
    '
}

case "${1:-}" in
--self-test)
    fail=0
    expect() {
        got="$(verdict "$1" "$2" "$3")"
        if [ "$got" != "$4" ]; then
            echo "::error::self-test: verdict($1, '$2', $3) returned '$got', expected '$4'" >&2
            fail=1
        fi
    }
    expect 2100 ""   2000 over
    expect 2000 ""   2000 within
    expect 1999 ""   2000 within
    expect 5000 5000 2000 exempt
    expect 5001 5000 2000 stale
    expect 100  300  2000 stale

    fixture="$(mktemp)"
    printf '// why\n\n/// doc\nfn one() {}\n/* block\n * star\n */\n   \nfn two() {}\n' > "$fixture"
    counted="$(code_lines "$fixture")"
    if [ "$counted" != "2" ]; then
        echo "::error::self-test: code_lines counted $counted lines of code, expected 2" >&2
        fail=1
    fi
    rm -f "$fixture"

    if [ "$fail" -eq 0 ]; then
        echo "self-test ok: over-threshold, stale exemption, and code-vs-comment counting all still fail where they should"
    fi
    exit "$fail"
    ;;
--update)
    # Re-record every exemption at its current size and drop the ones that no
    # longer need it. mv, not cat>: bash keeps reading this script through its
    # open descriptor, so the old inode must stay alive.
    self="$0"
    tmp="$(mktemp)"
    awk -v thr="$THRESHOLD" '
        function count_code(path,   line, s, n) {
            n = 0
            while ((getline line < path) > 0) {
                s = line; sub(/^[ \t]+/, "", s)
                if (s == "") continue
                if (s ~ /^\/\//) continue
                if (s ~ /^\/\*/) continue
                if (s ~ /^\*/) continue
                n++
            }
            close(path)
            return n
        }
        /^ALLOWLIST=\$\(cat <<.EOF.$/ { inblock = 1; print; next }
        inblock && /^EOF$/            { inblock = 0; print; next }
        inblock && $0 !~ /^[[:space:]]*#/ && NF {
            split($0, parts, "\t")
            path = parts[1]
            n = count_code(path)
            if (n <= thr) {
                pruned = pruned "\n  " path " — " n " code lines, exemption no longer needed"
                next
            }
            print path "\t" n
            next
        }
        { print }
        END { if (pruned != "") print pruned > "/dev/stderr" }
    ' "$self" > "$tmp" && mv "$tmp" "$self" && chmod +x "$self"
    echo "allowlist re-recorded: every exemption now asserts its current code-line count"
    exit 0
    ;;
"") ;;
*)
    echo "usage: $0 [--update|--self-test]" >&2
    exit 2
    ;;
esac

tracked="$(mktemp)"
git ls-files '*.rs' > "$tracked"

offenders=""
stale_entries=""
dangling=""
count_offenders=0
count_stale=0
count_dangling=0

while IFS= read -r f; do
    [ -f "$f" ] || continue
    actual="$(code_lines "$f")"
    asserted="$(asserted_for "$f")"
    case "$(verdict "$actual" "$asserted" "$THRESHOLD")" in
    over)
        offenders+="$(printf '%7d  %s\n' "$actual" "$f")"$'\n'
        count_offenders=$((count_offenders + 1))
        ;;
    stale)
        stale_entries+="$(printf '%7d  asserted %-7s %s\n' "$actual" "$asserted" "$f")"$'\n'
        count_stale=$((count_stale + 1))
        ;;
    esac
done < <(git ls-files '*.rs')

# An exemption for a path that no longer exists is an exemption nobody reviews.
while IFS=$'\t' read -r path asserted; do
    case "$path" in ''|'#'*) continue ;; esac
    if ! grep -Fxq "$path" "$tracked"; then
        dangling+="  $path"$'\n'
        count_dangling=$((count_dangling + 1))
    fi
done <<< "$ALLOWLIST"
rm -f "$tracked"

failed=0

if [ "$count_offenders" -gt 0 ]; then
    failed=1
    echo "::error::File size limit exceeded ($THRESHOLD lines of code)."
    echo ""
    echo "The following files are too large:"
    echo "$offenders"
fi

if [ "$count_stale" -gt 0 ]; then
    failed=1
    echo "::error::An allowlist entry records a code-line count the file no longer has ($count_stale)."
    echo ""
    echo "The exemption is re-asserted, not inherited, so a stale entry cannot"
    echo "quietly cover years of growth:"
    echo "$stale_entries"
    echo ""
    echo "Run '$0 --update' to re-record every entry, or split the file and drop"
    echo "its entry entirely."
fi

if [ "$count_dangling" -gt 0 ]; then
    failed=1
    echo "::error::An allowlist entry names a path that is not tracked ($count_dangling)."
    echo ""
    echo "$dangling"
    echo "Run '$0 --update' to drop it."
fi

if [ "$failed" -eq 0 ]; then
    echo "OK: no Rust source file exceeds $THRESHOLD lines of code, and every exemption asserts its current size."
    exit 0
fi

echo ""
echo "Split the offending files into topical sub-modules. See"
echo "v0.5.1019/v0.5.1020 commits on chore/split-large-files for"
echo "the recipe: extract function groups into sibling files,"
echo "re-export from mod.rs with explicit named use statements"
echo "(globs don't propagate through transitive re-exports). To"
echo "deliberately exempt a file, add it to the ALLOWLIST block"
echo "with its rationale and its code-line count."
exit 1
