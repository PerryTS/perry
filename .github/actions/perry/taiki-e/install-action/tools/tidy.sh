#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0 OR MIT
set -CeEuo pipefail
IFS=$'\n\t'
trap -- 'printf >&2 "%s\n" "${0##*/}: trapped SIGINT"; exit 1' SIGINT
cd -- "$(dirname -- "$0")"/..

# USAGE:
#    GITHUB_TOKEN=$(gh auth token) ./tools/tidy.sh
#
# Note: This script requires the following tools:
# - docker or podman (or compatible CLI specified by TIDY_DOCKER_PATH. when both available and TIDY_DOCKER_PATH is not set, docker is preferred)
#
# This script is shared by projects under github.com/taiki-e, so there may also
# be checks for files not included in this repository, but they will be skipped
# if the corresponding files do not exist.
# It is not intended for manual editing.

bail() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    printf '::error::%s\n' "$*"
  else
    printf >&2 'error: %s\n' "$*"
  fi
  exit 1
}

if [[ $# -gt 0 ]]; then
  cat <<EOF
USAGE:
    $0
EOF
  exit 1
fi

image='ghcr.io/taiki-e/tidy'
if [[ -n "${TIDY_DEV:-}" ]]; then
  image+=':latest'
else
  image+='@sha256:99d576935b91cbb7c97ad4b11b8888c5f36a0b8a720d2bab9a2ac3cc2a4b97c7'
fi
user="$(id -u):$(id -g)"
workdir="${PWD}"
tmp=$(mktemp -d)
trap -- 'rm -rf -- "${tmp:?}"' EXIT
mkdir -p -- "${tmp}"/{pwsh-cache,pwsh-local,zizmor-cache,dummy-dir,tmp}
printf '' >"${tmp}"/dummy
code=0
color=''
if [[ -t 1 ]] || [[ -n "${GITHUB_ACTIONS:-}" ]]; then
  color=1
fi
# Refs:
# - https://docs.docker.com/reference/cli/docker/container/run/
# - https://docs.podman.io/en/latest/markdown/podman-run.1.html
# - https://cheatsheetseries.owasp.org/cheatsheets/Docker_Security_Cheat_Sheet.html
common_args=(
  run --rm --init
  --cap-drop=all
  --security-opt=no-new-privileges
  --read-only
  --env GITHUB_ACTIONS
  --env CI
  --env CARGO_TERM_COLOR
  --env REMOVE_UNUSED_WORDS
  --env TIDY_COLOR_ALWAYS="${color}"
  --env TIDY_CALLER="$0"
  --env TIDY_EXPECTED_MARKDOWN_FILE_COUNT
  --env TIDY_EXPECTED_RUST_FILE_COUNT
  --env TIDY_EXPECTED_CLANG_FORMAT_FILE_COUNT
  --env TIDY_EXPECTED_PRETTIER_FILE_COUNT
  --env TIDY_EXPECTED_TOML_FILE_COUNT
  --env TIDY_EXPECTED_SHELL_FILE_COUNT
  --env TIDY_EXPECTED_DOCKER_FILE_COUNT
)
if [[ -n "${TIDY_DOCKER_PATH:-}" ]]; then
  docker="${TIDY_DOCKER_PATH}"
elif type -P docker >/dev/null; then
  docker='docker'
elif type -P podman >/dev/null; then
  docker='podman'
else
  bail 'this script requires docker or podman'
fi
rootless=''
if [[ "$("${docker}" --version)" == *'podman'* ]]; then
  if [[ "$("${docker}" info)" == *'rootless: true'* ]]; then
    rootless=1
  fi
elif [[ "$("${docker}" info -f '{{println .SecurityOptions}}')" == *'rootless'* ]]; then
  rootless=1
fi
if [[ -n "${rootless}" ]]; then
  printf 'docker path: %s\n' "${docker} (rootless)"
else
  printf 'docker path: %s\n' "${docker}"
  common_args+=(--user "${user}")
fi

# Map ignored files (e.g., .env) to dummy files.
while IFS= read -r path; do
  if [[ -d "${path}" ]]; then
    common_args+=(
      --mount "type=bind,source=${tmp}/dummy-dir,target=${workdir}/${path},readonly"
    )
  else
    common_args+=(
      --mount "type=bind,source=${tmp}/dummy,target=${workdir}/${path},readonly"
    )
  fi
done < <(git status --porcelain --ignored | grep -E '^!!' | cut -d' ' -f2)

docker_run() {
  local script="$1"
  shift
  "${docker}" "${common_args[@]}" "$@" "${image}" /checks/"${script}"
  code2="$?"
  if [[ ${code} -eq 0 ]] && [[ ${code2} -ne 0 ]]; then
    code="${code2}"
  fi
}

set +e
docker_run offline.sh \
  --mount "type=bind,source=${workdir},target=${workdir}" --workdir "${workdir}" \
  --mount "type=bind,source=${workdir}/.git,target=${workdir}/.git,readonly" \
  --mount "type=bind,source=${tmp}/tmp,target=/tmp/tidy" \
  --mount "type=bind,source=${tmp}/pwsh-cache,target=/.cache/powershell" \
  --mount "type=bind,source=${tmp}/pwsh-local,target=/.local/share/powershell" \
  --network=none
# Some good audits requires access to GitHub API.
docker_run zizmor.sh \
  --mount "type=bind,source=${workdir},target=${workdir},readonly" --workdir "${workdir}" \
  --mount "type=bind,source=${tmp}/zizmor-cache,target=/.cache/zizmor" \
  --env GH_TOKEN --env GITHUB_TOKEN --env ZIZMOR_GITHUB_TOKEN
# We use remote dictionary.
docker_run cspell.sh \
  --mount "type=bind,source=${workdir},target=${workdir},readonly" --workdir "${workdir}" \
  --mount "type=bind,source=${workdir}/.github/.cspell/project-dictionary.txt,target=${workdir}/.github/.cspell/project-dictionary.txt" \
  --mount "type=bind,source=${workdir}/.github/.cspell/rust-dependencies.txt,target=${workdir}/.github/.cspell/rust-dependencies.txt" \
  --mount "type=bind,source=${tmp}/tmp,target=/tmp/tidy"

exit "${code}"
