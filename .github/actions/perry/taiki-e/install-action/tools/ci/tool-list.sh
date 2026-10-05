#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0 OR MIT
set -CeEuo pipefail
IFS=$'\n\t'
trap -- 's=$?; printf >&2 "%s\n" "${0##*/}:${LINENO}: \`${BASH_COMMAND}\` exit with ${s}"; exit ${s}' ERR
cd -- "$(dirname -- "$0")"/../..

# They don't provide prebuilt binaries for musl or old glibc host.
# version `GLIBC_2.39' not found
glibc_pre_2_39_incompat=(
  cargo-crap
  cargo-deb
  cargo-mutants
  cargo-spellcheck
  zizmor
)
# version `GLIBC_2.35' not found
glibc_pre_2_35_incompat=(
  "${glibc_pre_2_39_incompat[@]}"
  zola
)
# version `GLIBC_2.34' not found
glibc_pre_2_34_incompat=(
  "${glibc_pre_2_35_incompat[@]}"
  cargo-sort
  espup
  wait-for-them
  xbuild
)
# version `GLIBC_2.31' not found
glibc_pre_2_31_incompat=(
  "${glibc_pre_2_34_incompat[@]}"
  cargo-vet
)
# version `GLIBC_2.28' not found
glibc_pre_2_28_incompat=(
  "${glibc_pre_2_31_incompat[@]}"
  wasm-tools
  wasmtime
)
# version `GLIBC_2.27' not found
glibc_pre_2_27_incompat=(
  "${glibc_pre_2_28_incompat[@]}"
  cargo-watch
  cyclonedx
  mdbook-linkcheck
  protoc
  valgrind
)
# version `GLIBC_2.17' not found
glibc_pre_2_17_incompat=(
  "${glibc_pre_2_27_incompat[@]}"
  deepsource # https://github.com/DeepSourceCorp/cli/issues/245
  rafn       # built on ubuntu-24.04, requires GLIBC_2.17+
  rust
)
musl_incompat=(
  "${glibc_pre_2_17_incompat[@]}"
)

incompat_tools=()
case "${1:-}" in
  '') version=latest ;;
  major.minor.patch | major.minor | major)
    version="$1"
    # Specifying the version of valgrind and cargo-binstall is not supported.
    incompat_tools+=(valgrind cargo-binstall)
    ;;
  *)
    printf 'tool=%s\n', "$1"
    exit 1
    ;;
esac
runner="${2:-}"
case "$(uname -s)" in
  Linux)
    host_os=linux
    ldd_version=$(ldd --version 2>&1 || true)
    if [[ "${ldd_version}" == *'musl'* ]]; then
      incompat_tools+=("${musl_incompat[@]}")
    else
      host_glibc_version=$(grep -E "GLIBC|GNU libc" <<<"${ldd_version}")
      host_glibc_version="${host_glibc_version##* }"
      higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.39"$'\n'"${host_glibc_version}" | tail -1)
      if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
        higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.35"$'\n'"${host_glibc_version}" | tail -1)
        if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
          higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.34"$'\n'"${host_glibc_version}" | tail -1)
          if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
            higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.31"$'\n'"${host_glibc_version}" | tail -1)
            if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
              higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.28"$'\n'"${host_glibc_version}" | tail -1)
              if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
                higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.27"$'\n'"${host_glibc_version}" | tail -1)
                if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
                  higher_glibc_version=$(LC_ALL=C sort -Vu <<<"2.17"$'\n'"${host_glibc_version}" | tail -1)
                  if [[ "${higher_glibc_version}" != "${host_glibc_version}" ]]; then
                    incompat_tools+=("${glibc_pre_2_17_incompat[@]}")
                  else
                    incompat_tools+=("${glibc_pre_2_27_incompat[@]}")
                  fi
                else
                  incompat_tools+=("${glibc_pre_2_28_incompat[@]}")
                fi
              else
                incompat_tools+=("${glibc_pre_2_31_incompat[@]}")
              fi
            else
              incompat_tools+=("${glibc_pre_2_34_incompat[@]}")
            fi
          else
            incompat_tools+=("${glibc_pre_2_35_incompat[@]}")
          fi
        else
          incompat_tools+=("${glibc_pre_2_39_incompat[@]}")
        fi
      fi
    fi
    if ! type -P snap >/dev/null; then
      incompat_tools+=(valgrind)
    fi
    if ! type -P apt-get >/dev/null || [[ "${runner}" == 'ubuntu:14.04' ]]; then
      incompat_tools+=(cyclonedx)
    fi
    if [[ "${runner}" == 'almalinux:10'* ]]; then
      incompat_tools+=(cargo-deb) # no dpkg in package manager
    fi
    case "${runner}" in
      debian/eol:jessie-slim | ubuntu:14.04 | alpine:3.2) incompat_tools+=(bpf-linker) ;; # no zstd in package manager
    esac
    ;;
  Darwin)
    host_os=macos
    if [[ "${runner}" == 'macos-15-intel' ]]; then
      incompat_tools+=(cargo-rdme martin release-plz) # no prebuilt binary
    fi
    ;;
  MINGW* | MSYS* | CYGWIN* | Windows_NT) host_os=windows ;;
  *) bail "unrecognized OS type '$(uname -s)'" ;;
esac
# NB: Sync with main.sh.
case "$(uname -m)" in
  aarch64 | arm64) host_arch=aarch64 ;;
  xscale | arm | armv*l) bail "32-bit Arm runner is not supported yet by this action; if you need support for this platform, please submit an issue at <https://github.com/taiki-e/install-action>" ;;
  ppc64le) host_arch=powerpc64le ;;
  riscv64) host_arch=riscv64 ;;
  s390x) host_arch=s390x ;;
  loongarch64 | mips | mips64 | ppc | ppc64 | sun4v) bail "$(uname -m) runner is not supported yet by this action; if you need support for this platform, please submit an issue at <https://github.com/taiki-e/install-action>" ;;
  # TODO: uname -m on windows-11-arm returns "x86_64"
  *) host_arch=x86_64 ;;
esac

tools=()
for manifest in tools/codegen/base/*.json; do
  tool_name="${manifest##*/}"
  tool_name="${tool_name%.*}"
  # cross -V requires rustc
  if [[ "${tool_name}" == "cross" ]] && ! type -P rustc >/dev/null; then
    continue
  fi
  case "${host_os}" in
    linux*)
      # cargo-apple-runner is not supported on Linux
      if [[ "${tool_name}" == "cargo-apple-runner" ]]; then
        continue
      fi
      if [[ "${host_arch}" != "x86_64" ]] && [[ "$(jq -r --arg p "${host_arch}_${host_os}_gnu" '.platform[$p]' "${manifest}")" == "null" ]] && [[ "$(jq -r --arg p "${host_arch}_${host_os}_musl" '.platform[$p]' "${manifest}")" == "null" ]]; then
        continue
      fi
      ;;
    *)
      if [[ "$(jq -r --arg p "x86_64_${host_os}" '.platform[$p]' "${manifest}")" == "null" ]] && [[ "$(jq -r --arg p "${host_arch}_${host_os}" '.platform[$p]' "${manifest}")" == "null" ]]; then
        continue
      fi
      ;;
  esac
  for incompat in ${incompat_tools[@]+"${incompat_tools[@]}"}; do
    if [[ "${incompat}" == "${tool_name}" ]]; then
      tool_name=''
      break
    fi
  done
  if [[ -n "${tool_name}" ]]; then
    if [[ "${version}" != "latest" ]]; then
      latest_version=$(jq -r '.latest.version' "manifests/${tool_name}.json")
      case "${version}" in
        major.minor.patch) tool_name+="@${latest_version}" ;;
        major.minor) tool_name+="@${latest_version%.*}" ;;
        major) tool_name+="@${latest_version%%.*}" ;;
        *) exit 1 ;;
      esac
    fi
    if [[ "${tool_name}" != *"@0" ]] && [[ "${tool_name}" != *"@0.0" ]]; then
      tools+=("${tool_name}")
    fi
  fi
done
if [[ "${version}" != "latest" ]]; then
  tools_tmp=()
  for tool in "${tools[@]}"; do
    tools_tmp+=("${tool}")
  done
  tools=("${tools_tmp[@]}")
fi

# Not manifest-based
case "${runner}" in
  # requires glibc 2.17 / musl 1.2
  centos:6 | alpine:3.2) ;;
  *)
    case $((RANDOM % 4)) in
      0) rust=rust ;;
      1) rust=rust@stable ;;
      2) rust=rust@nightly ;;
      3) rust=rust@1.93 ;;
    esac
    case $((RANDOM % 3)) in
      0) ;;
      1) rust+='+thumbv6m-none-eabi' ;;
      2) rust+=' + thumbv6m-none-eabi' ;;
    esac
    if [[ "${rust}" == *'nightly'* ]]; then
      component=miri
    else
      component=rustfmt
    fi
    case $((RANDOM % 5)) in
      0) ;;
      1) rust+="+${component}" ;;
      2) rust+=" + ${component}" ;;
      3) rust+=" +${component}" ;;
      4) rust+="+ ${component}" ;;
    esac
    case $((RANDOM % 3)) in
      0) ;;
      1) rust+='+thumbv7m-none-eabi' ;;
      2) rust+=' + thumbv7m-none-eabi' ;;
    esac
    tools+=("${rust}")
    ;;
esac
case "${host_os}" in
  linux*)
    # Installing snap to container is difficult...
    # Specifying the version of valgrind is not supported.
    if type -P snap >/dev/null && [[ "${version}" == "latest" ]]; then
      tools+=(valgrind)
    fi
    ;;
esac
# cargo-watch/watchexec-cli is supported by cargo-binstall (through quickinstall)
case "${version}" in
  latest) tools+=(cargo-watch watchexec-cli) ;;
  major.minor.patch) tools+=(cargo-watch@8.5.2 watchexec-cli@2.1.2) ;;
  major.minor) tools+=(cargo-watch@8.5 watchexec-cli@2.1) ;;
  major) tools+=(cargo-watch@8 watchexec-cli@2) ;;
  *) exit 1 ;;
esac

# sort and dedup
IFS=$'\n'
# shellcheck disable=SC2207
tools=($(LC_ALL=C sort -u <<<"${tools[*]}"))
IFS=$'\n\t'

comma_sep=$((RANDOM % 2))
list=''
for tool in "${tools[@]}"; do
  if [[ -n "${list}" ]]; then
    if [[ "${comma_sep}" == "1" ]]; then
      list+=','
    else
      case $((RANDOM % 2)) in
        0) list+=' ' ;;
        1) list+=$'\t' ;;
      esac
    fi
  fi
  case $((RANDOM % 3)) in
    0) ;;
    1) list+=' ' ;;
    2) list+=$' \t ' ;;
  esac
  list+="${tool}"
  case $((RANDOM % 3)) in
    0) ;;
    1) list+=' ' ;;
    2) list+=$' \t ' ;;
  esac
done
if [[ "${comma_sep}" == "1" ]]; then
  case $((RANDOM % 2)) in
    0) list+=',' ;;
  esac
fi
printf 'tool=%s\n' "${list}"
