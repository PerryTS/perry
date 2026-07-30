#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"

cat >"$TMP/bin/git" <<'SH'
#!/usr/bin/env bash
case "$1" in
  rev-parse) [[ "${FAKE_INVALID_BASE:-0}" != 1 ]] && printf '%s\n' deadbeef ;;
  diff)
    [[ " $* " != *' --diff-filter='* ]]
    printf '%s\n' 'crates/perry-parser/src/with space.rs' 'crates/perry-runtime/src/deleted.rs'
    ;;
  *) exit 1 ;;
esac
SH

cat >"$TMP/bin/python3" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
input="$(cat)"
case " ${*:-} " in
  *' --with-tests '*) [[ -z "$input" ]] || printf '%s\n' "$input" ;;
  *' --has-lib '*) [[ "$input" == perry-parser ]] ;;
  *)
    grep -qx 'crates/perry-parser/src/with space.rs' <<< "$input"
    grep -qx 'crates/perry-runtime/src/deleted.rs' <<< "$input"
    if [[ "${FAKE_RUNTIME_ONLY:-0}" == 1 ]]; then
      printf '%s\n' perry-runtime
    else
      printf '%s\n' perry-runtime perry perry-parser
    fi
    ;;
esac
SH

cat >"$TMP/bin/cargo" <<'SH'
#!/usr/bin/env bash
printf 'jobs=%s threads=%s argv=%s\n' \
  "${CARGO_BUILD_JOBS:-}" "${RUST_TEST_THREADS:-}" "$*" >>"$FAKE_CARGO_LOG"
SH
chmod +x "$TMP/bin/"*

PATH="$TMP/bin:/usr/bin:/bin" FAKE_CARGO_LOG="$TMP/cargo.log" \
  "$ROOT/scripts/test_affected_crates.sh" --base 'base with space' --dry-run \
  >"$TMP/dry-run.out"
grep -Fq 'Changed files since base with space:' "$TMP/dry-run.out"
grep -Fq 'env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test --lib -p perry-runtime' "$TMP/dry-run.out"
grep -Fq 'env CARGO_BUILD_JOBS=1 cargo test --bins -p perry' "$TMP/dry-run.out"
grep -Fq 'env CARGO_BUILD_JOBS=1 cargo test --lib --bins -p perry-parser' "$TMP/dry-run.out"
[[ ! -e "$TMP/cargo.log" ]]

PATH="$TMP/bin:/usr/bin:/bin" FAKE_CARGO_LOG="$TMP/cargo.log" FAKE_RUNTIME_ONLY=1 \
  "$ROOT/scripts/test_affected_crates.sh" --base HEAD --dry-run \
  >"$TMP/runtime-only.out"
grep -Fq 'cargo test --lib -p perry-runtime' "$TMP/runtime-only.out"
[[ "$(grep -c '^+ ' "$TMP/runtime-only.out")" -eq 1 ]]

PATH="$TMP/bin:/usr/bin:/bin" FAKE_CARGO_LOG="$TMP/cargo.log" \
  "$ROOT/scripts/test_affected_crates.sh" --base HEAD >"$TMP/run.out"
cat >"$TMP/expected.log" <<'EOF'
jobs=1 threads=1 argv=test --lib -p perry-runtime
jobs=1 threads= argv=test --bins -p perry
jobs=1 threads= argv=test --lib --bins -p perry-parser
EOF
cmp "$TMP/expected.log" "$TMP/cargo.log"

if PATH="$TMP/bin:/usr/bin:/bin" FAKE_INVALID_BASE=1 \
  "$ROOT/scripts/test_affected_crates.sh" --base missing >"$TMP/invalid.out" 2>"$TMP/invalid.err"; then
  echo "expected invalid base to fail" >&2
  exit 1
fi
grep -Fq 'error: invalid base revision: missing' "$TMP/invalid.err"

echo "affected crate runner: ok"
