#!/usr/bin/env bash
# Visual styling test ↔ spec consistency check (issue #185 follow-up).
#
# `docs/examples/ui/styling/visual_test.ts` is the comprehensive styling
# visual test app; `visual_test.spec.md` is its companion expected-values
# manifest used for LLM/human verification of screenshots. The two files
# need to stay in lockstep — adding a section to the .ts without
# documenting it in the spec (or vice versa) silently breaks the
# verification flow.
#
# This script enforces three invariants:
#   1. Both files exist.
#   2. The number of `labeled("N. ...", ...)` calls in the .ts matches
#      the number of `### N.` section headers in the spec.
#   3. At least one numbered section exists; an empty pair proves nothing.
#
# CI runs this after `run_ui_styling_matrix.sh` to keep the audit
# infrastructure + the visual-test infrastructure synchronized. The
# actual compile-test of `visual_test.ts` is handled by the existing
# `run_doc_tests.sh` loop on each platform (its `// platforms:` and
# `// targets:` headers route it to the right CI lanes).
#
# Usage:
#   scripts/run_visual_test_check.sh

set -uo pipefail

cd "$(dirname "$0")/.."

TS=docs/examples/ui/styling/visual_test.ts
SPEC=docs/examples/ui/styling/visual_test.spec.md

if [[ ! -f "$TS" ]]; then
    echo "FAIL: $TS missing"
    exit 1
fi

if [[ ! -f "$SPEC" ]]; then
    echo "FAIL: $SPEC missing"
    exit 1
fi

# Count `labeled("N. ...", ...)` lines in the .ts. These are the
# numbered section labels that wrap each row.
ts_sections=$(grep -cE '^\s*labeled\("[0-9]+\.' "$TS")

# Count `### N. ...` headers in the spec — the per-section detail
# blocks. Header level 3 is reserved for these; the meta sections
# at the top use level 2.
spec_sections=$(grep -cE '^### [0-9]+\.' "$SPEC")

if [[ "$ts_sections" -eq 0 && "$spec_sections" -eq 0 ]]; then
    echo "FAIL: visual test and spec need at least one numbered section"
    exit 1
fi

if [[ "$ts_sections" != "$spec_sections" ]]; then
    echo "FAIL: visual test sections drift detected"
    echo "  $TS has $ts_sections numbered labeled() calls"
    echo "  $SPEC has $spec_sections '### N.' section headers"
    echo
    echo "Either the test file added/removed a row without updating the"
    echo "spec, or vice versa. Walk both files top-to-bottom and align"
    echo "them — every labeled() row needs a matching '### N.' section"
    echo "in the spec with the per-cell expected visible signatures."
    exit 1
fi

# Sanity: each section number 1..N should appear exactly once in each file.
for ((i = 1; i <= ts_sections; i++)); do
    ts_hits=$(grep -cE "^\s*labeled\(\"${i}\." "$TS" || true)
    spec_hits=$(grep -cE "^### ${i}\." "$SPEC" || true)
    if [[ "$ts_hits" -ne 1 || "$spec_hits" -ne 1 ]]; then
        echo "FAIL: section $i appears $ts_hits times in $TS, $spec_hits in $SPEC (each must be 1)"
        exit 1
    fi
done

echo "OK: visual styling test ↔ spec in sync ($ts_sections sections)"
