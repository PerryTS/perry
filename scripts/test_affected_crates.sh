#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASE="${PERRY_TEST_BASE:-origin/main}"
DRY_RUN=false

usage() {
  echo "Usage: $0 [--base REV] [--dry-run]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ $# -ge 2 ]] || { echo "error: --base requires a revision" >&2; exit 2; }
      BASE="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

for tool in git python3 cargo sort grep sed env; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: required tool not found: $tool" >&2; exit 127; }
done

cd "$ROOT"
BASE_COMMIT="$(git rev-parse --verify --quiet --end-of-options "$BASE^{commit}")" || {
  echo "error: invalid base revision: $BASE" >&2
  exit 2
}

changed_files="$(git diff --name-only "$BASE_COMMIT" -- | LC_ALL=C sort -u)"
scope="$(printf '%s\n' "$changed_files" | python3 scripts/ci_test_scope.py)"

echo "Changed files since $BASE:"
printf '%s\n' "$changed_files"
echo "Packages in test scope:"
printf '%s\n' "$scope"
if [[ -z "$scope" ]]; then
  echo "No crates affected by this diff -- nothing to test."
  exit 0
fi

run() {
  printf '+ '
  printf '%q ' "$@"
  printf '\n'
  "$DRY_RUN" || "$@"
}

if printf '%s\n' "$scope" | grep -qx 'perry-runtime'; then
  run env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test --lib -p perry-runtime
fi

rest="$(printf '%s\n' "$scope" | sed '/^perry-runtime$/d' | python3 scripts/ci_test_scope.py --with-tests)"
echo "Crates with unit tests in scope:"
printf '%s\n' "$rest"
while IFS= read -r package; do
  [[ -n "$package" ]] || continue
  if printf '%s\n' "$package" | python3 scripts/ci_test_scope.py --has-lib; then
    run env CARGO_BUILD_JOBS=1 cargo test --lib --bins -p "$package"
  else
    run env CARGO_BUILD_JOBS=1 cargo test --bins -p "$package"
  fi
done <<< "$rest"
