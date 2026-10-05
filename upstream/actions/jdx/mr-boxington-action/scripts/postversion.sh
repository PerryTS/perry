#!/usr/bin/env bash
set -euxo pipefail

version=$(jq -r .version package.json)
major_version=${version%%.*}

gh auth setup-git

git tag "v$version" || echo "Tag v$version already exists locally"
git push origin "v$version" || echo "Tag v$version already exists on remote"

git tag "v$major_version" -f
if ! git push origin "v$major_version" -f; then
  echo "Failed to push v$major_version tag, fetching and retrying"
  git fetch origin "refs/tags/v$major_version:refs/tags/v$major_version" -f
  git tag "v$major_version" -f
  git push origin "v$major_version" -f
fi

if gh release view "v$version" >/dev/null 2>&1; then
  echo "Release v$version already exists, skipping creation"
else
  gh release create "v$version" --generate-notes --verify-tag
fi
