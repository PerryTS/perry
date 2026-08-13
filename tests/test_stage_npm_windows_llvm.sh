#!/usr/bin/env bash
# Regression for #7985: the Windows release and npm artifacts must keep the
# dynamically linked LLVM-C.dll beside perry.exe.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FIXTURE_REPO="$WORK/repo"
ARTIFACTS="$WORK/artifacts"
WIN_ARTIFACT="$ARTIFACTS/perry-windows-x86_64"

mkdir -p \
  "$FIXTURE_REPO/scripts" \
  "$FIXTURE_REPO/npm/perry/bin" \
  "$FIXTURE_REPO/npm/perry-win32-x64" \
  "$WIN_ARTIFACT"

cp "$REPO_ROOT/scripts/stage-npm.sh" "$FIXTURE_REPO/scripts/stage-npm.sh"
cp "$REPO_ROOT/npm/perry/package.json.tmpl" "$FIXTURE_REPO/npm/perry/package.json.tmpl"
cp "$REPO_ROOT/npm/perry/bin/perry.js" "$FIXTURE_REPO/npm/perry/bin/perry.js"
cp "$REPO_ROOT/npm/perry-win32-x64/package.json.tmpl" \
  "$FIXTURE_REPO/npm/perry-win32-x64/package.json.tmpl"

printf '[workspace.package]\nversion = "0.0.0-test"\n' > "$FIXTURE_REPO/Cargo.toml"
printf 'fixture executable\n' > "$WIN_ARTIFACT/perry.exe"
printf 'fixture LLVM runtime\n' > "$WIN_ARTIFACT/LLVM-C.dll"

SKIP_MISSING=1 PERRY_NPM_NO_COMPRESS=1 \
  "$FIXTURE_REPO/scripts/stage-npm.sh" "$ARTIFACTS" > "$WORK/stage.out"

cmp "$WIN_ARTIFACT/perry.exe" \
  "$FIXTURE_REPO/npm/perry-win32-x64/bin/perry.exe"
cmp "$WIN_ARTIFACT/LLVM-C.dll" \
  "$FIXTURE_REPO/npm/perry-win32-x64/bin/LLVM-C.dll"

# The negative direction is load-bearing: a missing DLL must stop publishing,
# not silently produce an npm package whose perry.exe dies with 0xC0000135.
rm "$WIN_ARTIFACT/LLVM-C.dll"
if SKIP_MISSING=1 PERRY_NPM_NO_COMPRESS=1 \
    "$FIXTURE_REPO/scripts/stage-npm.sh" "$ARTIFACTS" \
    > "$WORK/missing.out" 2> "$WORK/missing.err"; then
  echo "stage-npm accepted a Windows artifact without LLVM-C.dll" >&2
  exit 1
fi
grep -F 'missing LLVM-C.dll required by perry.exe (#7985)' "$WORK/missing.err" >/dev/null

echo "Windows LLVM runtime npm staging: OK (copy + missing-DLL refusal)"
