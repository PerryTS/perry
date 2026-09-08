#!/usr/bin/env bash
# #1112 — publish perry-ffi to crates.io. Maintainer-only: needs an
# `~/.cargo/credentials.toml` with a crates.io API token (run
# `cargo login` once if missing).
#
# **Prerequisites — publish dependency packages before perry-ffi.** The
# required perry-native-registration dependency and the optional perry-runtime
# dependency (runtime-link) must both resolve on crates.io. Cargo checks the
# optional edge even when external wrappers leave that feature off.
# For dependency versions not already available, the order is:
#
#   1. cargo publish --dry-run -p perry-native-registration
#      cargo publish -p perry-native-registration
#   2. cargo publish -p perry-runtime
#      (Its own workspace dependencies need prior publication as appropriate;
#      dependency publication remains a manual maintainer prerequisite.)
#   3. ./scripts/publish_perry_ffi.sh                 (perry-ffi dry run)
#      ./scripts/publish_perry_ffi.sh --really-publish (perry-ffi publication)
#
# Run from the workspace root. Ships whatever the
# `[workspace.package].version` currently in `Cargo.toml` says, so
# make sure the changeset + Cargo.toml were updated for this release
# first (the standard workflow already covers that).
set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$WORKSPACE_ROOT"

VERSION="$(grep -E '^version = "0\.5\.' Cargo.toml | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
echo "Workspace version: ${VERSION}"
echo "Prerequisites: publish perry-native-registration, then perry-runtime, before perry-ffi."
echo "The dependency versions required by Cargo.toml must already resolve on crates.io."

# Verify the package builds and would publish cleanly. `--allow-dirty`
# is fine because this script is meant to be run from a clean main
# branch right after a release commit lands — at that point the
# worktree may still have generated CHANGELOG / Cargo.lock changes
# from the auto-optimize pass, which cargo treats as dirty.
echo "===> cargo publish --dry-run -p perry-ffi"
LOG=/tmp/perry-ffi-publish-dry.log
if ! cargo publish --dry-run -p perry-ffi --allow-dirty 2>&1 | tee "$LOG"; then
  if grep -q 'no matching package named `perry-native-registration`' "$LOG"; then
    echo
    echo "ERROR: perry-ffi requires perry-native-registration on crates.io."
    echo "       First run cargo publish --dry-run -p perry-native-registration,"
    echo "       then cargo publish -p perry-native-registration."
    echo "       Ensure perry-runtime also resolves, then re-run this script."
    exit 2
  fi
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
