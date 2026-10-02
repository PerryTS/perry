#!/usr/bin/env bash
# `super.m` / `super.x` read the home object's current [[Prototype]],
# `instanceof` walks the live chain, and `C.prototype.__proto__ = X` relinks
# like Object.setPrototypeOf, after patches, deletes, accessors and relinks
# (expected.txt is node 26.5.1's output). Two trip counts, under forced
# evacuation.
set -euo pipefail
binary=$(realpath "${1:?pass the compiled fixture executable}")
fixture_dir=$(cd "$(dirname "$0")" && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT
for n in 40 41; do
  PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 "$binary" "$n" > "$tmp_dir/out" 2> "$tmp_dir/err"
  cmp "$fixture_dir/expected.txt" "$tmp_dir/out"
done
echo ok
