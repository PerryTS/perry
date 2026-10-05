#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0 OR MIT
set -CeEuo pipefail
IFS=$'\n\t'
trap -- 's=$?; printf >&2 "%s\n" "${0##*/}:${LINENO}: \`${BASH_COMMAND}\` exit with ${s}"; exit ${s}' ERR
cd -- "$(dirname -- "$0")"/..

# Update manifests
#
# USAGE:
#    ./tools/manifest.sh [PACKAGE [VERSION_REQ]]
#    ./tools/manifest.sh full

if [[ $# -eq 1 ]] && [[ "$1" == "full" ]]; then
  for manifest in tools/codegen/base/*.json; do
    package="${manifest##*/}"
    package="${package%.*}"
    cargo run --manifest-path tools/codegen/Cargo.toml --release -- "${package}"
  done
fi

if [[ $# -gt 0 ]]; then
  cargo run --manifest-path tools/codegen/Cargo.toml --release -- "$@"
  exit 0
fi

for manifest in tools/codegen/base/*.json; do
  package="${manifest##*/}"
  package="${package%.*}"
  cargo run --manifest-path tools/codegen/Cargo.toml --release -- "${package}" latest
done
