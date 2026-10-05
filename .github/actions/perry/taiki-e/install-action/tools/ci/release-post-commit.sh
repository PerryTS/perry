#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0 OR MIT
set -CeEuo pipefail
IFS=$'\n\t'
trap -- 's=$?; printf >&2 "%s\n" "${0##*/}:${LINENO}: \`${BASH_COMMAND}\` exit with ${s}"; exit ${s}' ERR
cd -- "$(dirname -- "$0")"/../..

tools=()
for tool in tools/codegen/base/*.json; do
  tool="${tool##*/}"
  tools+=("${tool%.*}")
done
# Aliases.
# NB: Update case for aliases in main.sh, tool input option in test-alias job
# in .github/workflows/ci.yml, and match for alias for tools/codegen/src/tools-markdown.rs.
tools+=(
  nextest
  wild-linker
  taplo-cli
  typos-cli
  wasm-bindgen-cli
  wasmtime-cli
)
# Non-manifest-based tools.
tools+=(
  rust
  valgrind
)

for tool in "${tools[@]}"; do
  (
    set -x
    git checkout -b "releases/${tool}"
    sed -Ei action.yml \
      -e "s/required: true/required: false/g" \
      -e "s/# default: #publish:tool/default: ${tool}/g"
    git add action.yml
    git commit -m "${tool}"
    git tag -f "${tool}"
    git checkout main
  )
  refs+=",+refs/heads/releases/${tool},+refs/tags/${tool}"
done

set -x

# Copy manifests to tmp dir.
manifests=/tmp/manifests
rm -rf -- "${manifests}"
mkdir -p -- "${manifests}"
cp -- ./manifests/* "${manifests}"

# Checkout manifest-schema branch
schema_version="$(grep -Eo "^version = \".*\" #publish:version" tools/manifest-schema/Cargo.toml)"
schema_version="$(cut -d\" -f2 <<<"${schema_version}")"
if [[ "${schema_version}" == '0.'* ]]; then
  schema_version="0.$(cut -d. -f2 <<<"${schema_version}")"
else
  schema_version="$(cut -d. -f1 <<<"${schema_version}")"
fi
schema_branch="manifest-schema-${schema_version}"
refs+=",refs/heads/${schema_branch}"

if git fetch origin "${schema_branch}"; then
  git checkout "origin/${schema_branch}" -B "${schema_branch}"
elif ! git checkout "${schema_branch}"; then
  # New branch with no history. Credit: https://stackoverflow.com/a/13969482
  git checkout --orphan "${schema_branch}"
  git rm -rf -- . || true
  git commit -m 'Initial commit' --allow-empty
fi

# Copy over schema
cp -- "${manifests}"/* ./

# Stage changes
git add .
# Detect changes, then commit and push if changes exist
if [[ "$(git status --porcelain=v1 | LC_ALL=C wc -l)" != "0" ]]; then
  git commit -m 'Update manifest schema'
fi
git checkout main

printf 'additional-refs: %s\n' "${refs}"
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'additional-refs=%s\n' "${refs}" >>"${GITHUB_OUTPUT}"
fi
