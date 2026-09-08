#!/usr/bin/env bash
# Compare the Tokio crate instances in the actual shipping archives.
set -euo pipefail

dist_dir="${1:?usage: check_release_tokio.sh <archive-directory>}"

tokio_ids() {
  local members
  # An unreadable archive is an error, distinct from a valid CPU-only wrapper.
  members=$(ar t "$1") || return 1
  # rustc may prefix members with a leaf crate. Split on dots as the link-time
  # checker does; consume every member, and never discard a second Tokio ID.
  printf '%s\n' "$members" | awk -F. '{
    for (i = 1; i <= NF; i++)
      if ($i ~ /^tokio-[0-9a-f]+$/) print $i
  }' | sort -u
}

want=$(tokio_ids "$dist_dir/libperry_stdlib.a")
if [ -z "$want" ] || [[ "$want" == *$'\n'* ]]; then
  echo "::error::stdlib must bundle exactly one Tokio instance; found: ${want:-none}" >&2
  exit 1
fi
echo "stdlib bundles $want"
checked=0
bad=0
for archive in "$dist_dir"/libperry_ext_*.a; do
  [ -e "$archive" ] || continue
  got=$(tokio_ids "$archive")
  if [ -z "$got" ]; then continue; fi
  checked=$((checked + 1))
  if [ "$got" != "$want" ]; then
    echo "::error::$(basename "$archive") bundles $got; stdlib bundles $want" >&2
    bad=$((bad + 1))
  fi
done
if [ "$checked" -eq 0 ]; then
  echo "::error::no Tokio-using ext archive was compared in $dist_dir" >&2
  exit 1
fi
if [ "$bad" -ne 0 ]; then
  echo "::error::$bad ext archive(s) disagree with stdlib's Tokio" >&2
  exit 1
fi
echo "::notice::$checked Tokio-using ext archive(s) all bundle $want"
