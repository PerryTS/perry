#!/usr/bin/env bash
# One feature union for the compiler, static wrappers and governed extensions.
set -euo pipefail
target="${1:?usage: build_release_ext.sh <rust-target>}"
packages=$(bash scripts/release_ext_packages.sh)
pkg_args=()
while IFS= read -r package; do
  if [ -n "$package" ]; then pkg_args+=(-p "$package"); fi
done <<< "$packages"
if [ "${#pkg_args[@]}" -eq 0 ]; then
  echo "::error::release extension inventory is empty" >&2
  exit 1
fi
cargo build --profile dist --target "$target" \
  -p perry -p perry-runtime-static -p perry-stdlib-static "${pkg_args[@]}"
echo "::notice::built $((${#pkg_args[@]} / 2)) ext packages in one coherent cargo invocation"
dist_dir="${CARGO_TARGET_DIR:-target}/$target/dist"
missing=0
while IFS= read -r package; do
  [ -n "$package" ] || continue
  archive="$dist_dir/lib${package//-/_}.a"
  if [ ! -s "$archive" ]; then
    echo "::error::governed extension produced no shipping archive: $archive" >&2
    missing=$((missing + 1))
  fi
done <<< "$packages"
if [ "$missing" -ne 0 ]; then
  echo "::error::$missing governed extension archive(s) are missing" >&2
  exit 1
fi
bash scripts/check_release_tokio.sh "$dist_dir"
