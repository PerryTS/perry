#!/usr/bin/env bash
#
# CI gate: fail if any tracked Rust source file exceeds the LOC threshold.
#
# Big single-file modules are hard to read, hard to review, and hurt
# build incrementality (touching one symbol invalidates the IDE +
# cargo-check work for thousands of lines downstream). This script
# enforces an upper bound and is run on every PR.
#
# Comments and blank lines do not count; test code remains review surface.
# Each exemption records an exact code-line count and a split rationale.
# Stale or untracked entries fail. --update refreshes counts and retires
# entries below the cap. Vendored snapshots retain upstream structure.
#
set -euo pipefail

THRESHOLD="${PERRY_FILE_SIZE_THRESHOLD:-2000}"

# Allowlist (one file per line as `<path><TAB><count>`; blank lines + `#`
# comments OK; `--update` rewrites it).
ALLOWLIST=$(cat <<'EOF'
# The module compilation pipeline shares closure state; decomposition is tracked in #1435.
crates/perry/src/commands/compile/run_pipeline.rs	6180
# Generator for-of destructuring remains in the statement-lowering core; split the function group in a follow-up.
crates/perry-hir/src/lower_decl/body_stmt.rs	2063
# Representation-aware codegen core; further decomposition is tracked in #1435.
crates/perry-codegen/src/codegen/mod.rs	3565
# Representation-aware HIR fact collection; further decomposition is tracked in #1435.
crates/perry-codegen/src/collectors/hir_facts.rs	2760
# Representation-aware expression dispatch; further decomposition is tracked in #1435.
crates/perry-codegen/src/expr/mod.rs	2826
# Loop versioning and representation evidence share lowering state; decomposition is tracked in #1435.
crates/perry-codegen/src/stmt/loops.rs	8290
# Representation-aware map core; further decomposition is tracked in #1435.
crates/perry-runtime/src/map.rs	3040
# Representation-aware set core; further decomposition is tracked in #1435.
crates/perry-runtime/src/set.rs	2347
# Representation feedback core; further decomposition is tracked in #1435.
crates/perry-runtime/src/typed_feedback.rs	2436
# Native ABI proof cases; test organization is a separate follow-up.
crates/perry-codegen/tests/native_proof_regressions.rs	15680
# Representation feedback regression cases; test organization is a separate follow-up.
crates/perry-runtime/src/typed_feedback/tests.rs	2289
# Lowering report schema and its emitters; split emitters in a follow-up.
crates/perry/src/commands/compile/lowering_report.rs	2321
# CallInliner argument and return remapping share one walker; split the traversal in a follow-up.
crates/perry-transform/src/inline/call_inliner.rs	2120
# Web Streams controllers share state; extract reader, tee and BYOB groups in a follow-up.
crates/perry-stdlib/src/streams.rs	2409
# Collection policy and debt accounting share phase state; decomposition is tracked in #1435.
crates/perry-runtime/src/gc/policy.rs	2860
# Shape registry table and publication invariants require a coordinated split.
crates/perry-runtime/src/object/shapes.rs	4926
# Qualified Turnloop snapshot; preserve upstream layout (third_party/turnloop/VENDORED.md).
third_party/turnloop/turnloop/src/driver.rs	2287
# Generated upstream WinRT collections bindings; preserve the vendor snapshot.
third_party/windows-winui/windows-collections/src/bindings.rs	3094
# Generated upstream WinRT async bindings; preserve the vendor snapshot.
third_party/windows-winui/windows-future/src/bindings.rs	2015
# Generated upstream WinUI bindings; preserve the vendor snapshot.
third_party/windows-winui/windows-reactor/src/bindings.rs	26441
# Upstream Windows Reactor backend; preserve layout for vendor comparisons.
third_party/windows-winui/windows-reactor/src/winui/backend/mod.rs	4035
EOF
)

# Anchor at repo root so the script can be invoked from anywhere.
cd "$(git rev-parse --show-toplevel)"

# The gate and --update share one lexer so recorded counts match validation.
# Rust block comments nest; a leading * outside a comment is executable code.
COUNT_CODE_AWK='
    function count_code(path,   line, n, depth, quoted, escaped, raw, endraw, i, c, pair, rest, code) {
        n = depth = quoted = escaped = 0
        raw = ""
        while ((getline line < path) > 0) {
            code = 0
            for (i = 1; i <= length(line); i++) {
                c = substr(line, i, 1)
                pair = substr(line, i, 2)
                if (depth) {
                    if (pair == "/*") { depth++; i++ }
                    else if (pair == "*/") { depth--; i++ }
                    continue
                }
                if (raw != "") {
                    code = 1
                    if (substr(line, i, length(raw)) == raw) {
                        i += length(raw) - 1
                        raw = ""
                    }
                    continue
                }
                if (quoted) {
                    code = 1
                    if (escaped) escaped = 0
                    else if (c == "\\") escaped = 1
                    else if (c == "\"") quoted = 0
                    continue
                }
                if (pair == "//") break
                if (pair == "/*") { depth++; i++; continue }
                if (c ~ /[[:space:]]/) continue
                code = 1
                rest = substr(line, i)
                if (c == "r" && match(rest, /^r#*"/)) {
                    raw = "\"" substr(rest, 2, RLENGTH - 2)
                    i += RLENGTH - 1
                } else if (c == "\"") quoted = 1
                else if (c == sprintf("%c", 39) && match(rest, /^\047([^\047\\]|\\.)\047/)) {
                    i += RLENGTH - 1
                }
            }
            if (code) n++
            escaped = 0
        }
        close(path)
        return n
    }
'

code_lines() {
    awk "$COUNT_CODE_AWK"'BEGIN { printf "%d", count_code(ARGV[1]); exit }' "$1"
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
    printf 'fn increment(p: &mut u32) {\n    *p += 1;\n}\n' > "$fixture"
    counted="$(code_lines "$fixture")"
    if [ "$counted" != "3" ]; then
        echo "::error::self-test: dereference code counted $counted lines, expected 3" >&2
        fail=1
    fi
    printf '/* block\nplain continuation\n/* nested */\n*/\nfn one() {}\n' > "$fixture"
    counted="$(code_lines "$fixture")"
    if [ "$counted" != "1" ]; then
        echo "::error::self-test: block comments counted $counted lines, expected 1" >&2
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
    awk -v thr="$THRESHOLD" "$COUNT_CODE_AWK"'
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
