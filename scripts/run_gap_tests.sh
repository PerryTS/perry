#!/usr/bin/env bash
# Committed runner for the Perry "gap" suite.
#
# Every test-files/test_gap_*.ts is AOT-compiled by Perry and diffed
# byte-for-byte against `node --experimental-strip-types`. This is a thin
# wrapper over run_parity_tests.sh --filter test_gap_ so it reuses the ONE
# canonical normalizer, the skip-list, the per-test output cap, and the JSON
# report (this shared-normalizer reuse is the seed of roadmap initiative I-14).
#
# Replaces the out-of-repo /tmp/run_gap_tests.sh that CLAUDE.md used to point
# at — the gap suite is the highest-signal-per-second test Perry has and was
# previously dark in CI.
#
# Regression-gate semantics: exits non-zero if any gap test fails parity or
# compilation and is NOT already triaged in test-parity/known_failures.json.
# (run_parity_tests.sh's own exit code only trips below 80% AGGREGATE parity,
# which is far too loose to catch a single-feature regression — exactly the
# "a module silently went to 0 behind a green build" class.)
#
# Requirements:
#   - a Rust toolchain (the wrapped run_parity_tests.sh builds target/release/perry)
#   - node with --experimental-strip-types
#   - jq
#
# Usage: scripts/run_gap_tests.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

echo "==> Running gap suite (test-files/test_gap_*.ts) via run_parity_tests.sh --filter test_gap_"
# run_parity_tests.sh exits 1 when AGGREGATE parity < 80%. We gate on "no NEW
# untriaged failures" instead (below), so don't let its aggregate exit abort us.
set +e
./run_parity_tests.sh --filter test_gap_
set -e

REPORT="test-parity/reports/latest.json"
KNOWN="test-parity/known_failures.json"
if [[ ! -f "$REPORT" ]]; then
  echo "ERROR: parity report not found at $REPORT (did run_parity_tests.sh run?)" >&2
  exit 2
fi

# Every failure in this report is a gap test (we filtered on test_gap_), so the
# whole failure set is the gap failure set. Drop empty entries (run_parity_tests.sh
# emits compile: [""] when there are zero compile failures).
jq -r '(.failures.parity // []) + (.failures.compile // []) | .[] | select(. != "")' \
  "$REPORT" | sort -u > /tmp/gap_all_fails.txt

if [[ -f "$KNOWN" ]]; then
  # known_failures.json is keyed by test name; skip the audit-metadata _schema key.
  jq -r 'keys[] | select(. != "_schema")' "$KNOWN" | sort -u > /tmp/gap_known.txt
else
  : > /tmp/gap_known.txt
fi

comm -23 /tmp/gap_all_fails.txt /tmp/gap_known.txt > /tmp/gap_new.txt
TOTAL=$(wc -l < /tmp/gap_all_fails.txt | tr -d ' ')

if [[ -s /tmp/gap_new.txt ]]; then
  echo "" >&2
  echo "NEW gap failures (not triaged in test-parity/known_failures.json):" >&2
  sed 's/^/  - /' /tmp/gap_new.txt >&2
  echo "" >&2
  echo "Fix the regression, or — if the failure is intentional/known — add a" >&2
  echo "triaged entry to test-parity/known_failures.json (category + reason)." >&2
  exit 1
fi

echo "All ${TOTAL} gap failures (if any) are known/triaged. Gap gate OK."
