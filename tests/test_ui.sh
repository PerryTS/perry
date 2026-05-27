#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

for test in "$ROOT"/tests/ui/*.ts; do
  [ -f "$test" ] || continue
  node --disable-warning=ExperimentalWarning --experimental-strip-types "$test"
  echo "PASS ui/$(basename "$test" .ts)"
done
