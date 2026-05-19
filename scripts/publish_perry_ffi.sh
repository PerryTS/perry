#!/usr/bin/env bash
# #1112 — publish perry-ffi to crates.io. Maintainer-only: needs an
# `~/.cargo/credentials.toml` with a crates.io API token (run
# `cargo login` once if missing).
#
# **Prerequisite — publish perry-runtime first.** perry-ffi has an
# `optional` dep on perry-runtime gated by the `runtime-link`
# feature (used by every in-tree `perry-ext-*` test crate). cargo
# publish rejects the perry-ffi package until the matching
# perry-runtime version exists on crates.io, even though external
# (npm-distributed) consumers will leave the feature off and never
# pull perry-runtime in. Order is:
#
#   1. cargo publish -p perry-runtime    (only the first time the
#                                         version's not yet on
#                                         crates.io; perry-runtime
#                                         itself currently depends
#                                         on other workspace crates
#                                         that would need similar
#                                         publish handling — out of
#                                         scope for this script).
#   2. ./scripts/publish_perry_ffi.sh    (this script).
#
# Run from the workspace root. Ships whatever the
# `[workspace.package].version` currently in `Cargo.toml` says, so
# make sure CHANGELOG.md + Cargo.toml were bumped for this release
# first (the standard workflow already covers that).
set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$WORKSPACE_ROOT"

VERSION="$(grep -E '^version = "0\.5\.' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
echo "Workspace version: ${VERSION}"

# Verify the package builds and would publish cleanly. `--allow-dirty`
# is fine because this script is meant to be run from a clean main
# branch right after a release commit lands — at that point the
# worktree may still have generated CHANGELOG / Cargo.lock changes
# from the auto-optimize pass, which cargo treats as dirty.
echo "===> cargo publish --dry-run -p perry-ffi"
LOG=/tmp/perry-ffi-publish-dry.log
if ! cargo publish --dry-run -p perry-ffi --allow-dirty 2>&1 | tee "$LOG"; then
  if grep -q "no matching package named \`perry-runtime\`" "$LOG"; then
    echo
    echo "ERROR: perry-ffi can't be published until perry-runtime ${VERSION}"
    echo "       is on crates.io. perry-ffi has an optional dep on it"
    echo "       (gated by the \`runtime-link\` feature, used by every"
    echo "       in-tree perry-ext-* test crate), and cargo publish"
    echo "       rejects the unresolvable reference."
    echo
    echo "       Publish perry-runtime first, then re-run this script."
    exit 2
  fi
  exit 1
fi

if [ "${1:-}" = "--really-publish" ]; then
  echo "===> cargo publish -p perry-ffi (live)"
  cargo publish -p perry-ffi --allow-dirty
  echo
  echo "perry-ffi ${VERSION} uploaded. Confirm at:"
  echo "  https://crates.io/crates/perry-ffi/${VERSION}"
else
  echo
  echo "Dry run complete. To actually upload, re-run with --really-publish:"
  echo "  $0 --really-publish"
fi
