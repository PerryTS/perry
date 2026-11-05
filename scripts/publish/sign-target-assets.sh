#!/usr/bin/env bash
# Shared by publication and the packaging dry run: use the packed CLI and bytes.
set -euo pipefail
archive=$1
packages=$2
assets=$3
: "${PERRY_CLI_UPDATE_SIGNING_KEY:?configure the 32-byte base64 Ed25519 signing secret}"
: "${PERRY_CLI_UPDATE_KEY_ID:?configure the matching trusted public-key id}"
: "${REPO:?release repository}"
: "${TAG:?release tag}"
work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/target-signing.XXXXXX")
trap 'rm -rf "$work"' EXIT
args=(--version "${RELEASE_VERSION:-}")
if [ "${VERIFY_NPM:-false}" = true ]; then args+=(--verify-npm); fi
# Complete selection and verification before signing any package.
python3 scripts/publish/select-target-assets.py "$packages" "${args[@]}" > "$work/packages.tsv"
mkdir -p "$assets"
# The host archive is written with `tar czf ... *` from the staging directory.
tar xzf "$archive" -C "$work" perry
while IFS=$'\t' read -r tgz kind triple version package; do
  name="${package}-${version}.tgz"
  cp "$tgz" "$assets/$name"
  (cd "$assets" && shasum -a 256 "$name" > "$name.sha256")
  kind_args=()
  if [ "$kind" = ext ]; then kind_args=(--ext); fi
  "$work/perry" updater sign-target-manifest ${kind_args[@]+"${kind_args[@]}"} \
    --artifact "$assets/$name" --triple "$triple" \
    --url "https://github.com/${REPO}/releases/download/${TAG}/${name}" \
    --release-version "$version" --key-id "$PERRY_CLI_UPDATE_KEY_ID" \
    --secret-key-b64 "$PERRY_CLI_UPDATE_SIGNING_KEY" > "$assets/$name.update.json"
  echo "signed $name"
done < "$work/packages.tsv"
