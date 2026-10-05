#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0 OR MIT
set -CeEuo pipefail
IFS=$'\n\t'

rx() {
  (
    set -x
    "$@"
  )
}
g() {
  IFS=' '
  local cmd="$*"
  IFS=$'\n\t'
  printf '::group::%s\n' "${cmd#retry }"
  "$@"
  printf '::endgroup::\n'
}
retry() {
  for i in {1..10}; do
    if "$@"; then
      return 0
    else
      sleep "${i}"
    fi
  done
  "$@"
}
bail() {
  printf '::error::install-action: %s\n' "$*"
  exit 1
}
warn() {
  printf '::warning::install-action: %s\n' "$*"
}
info() {
  printf >&2 'info: %s\n' "$*"
}
normalize_comma_or_space_separated() {
  # Normalize whitespace characters into space because it's hard to handle single input contains lines with POSIX sed alone.
  local list="${1//[$'\r\n\t']/ }"
  if [[ "${list}" == *","* ]]; then
    # If a comma is contained, consider it is a comma-separated list.
    # Drop leading and trailing whitespaces in each element.
    sed -E 's/ *\+ */+/g; s/ *, */,/g; s/^.//; s/,,$/,/' <<<",${list},"
  else
    # Otherwise, consider it is a whitespace-separated list.
    # Convert whitespace characters into comma.
    sed -E 's/ *\+ */+/g; s/ +/,/g; s/^.//' <<<" ${list} "
  fi
}
_sudo() {
  if type -P sudo >/dev/null; then
    sudo "$@"
  else
    "$@"
  fi
}
download_and_checksum() {
  local url="$1"
  local checksum="$2"
  if [[ -z "${enable_checksum}" ]]; then
    checksum=''
  fi
  info "downloading ${url}"
  retry curl --proto '=https' --tlsv1.2 -fsSL --retry 10 -o tmp "${url}"
  if [[ -n "${checksum}" ]]; then
    info "verifying sha256 checksum for $(basename -- "${url}")"
    if type -P sha256sum >/dev/null; then
      sha256sum -c - >/dev/null <<<"${checksum} *tmp"
    elif type -P shasum >/dev/null; then
      # GitHub-hosted macOS runner does not install GNU Coreutils by default.
      # https://github.com/actions/runner-images/issues/90
      shasum -a 256 -c - >/dev/null <<<"${checksum} *tmp"
    else
      bail "checksum requires 'sha256sum' or 'shasum' command; consider installing one of them or setting 'checksum' input option to 'false'"
    fi
  fi
}
download_and_extract() {
  local url="$1"
  shift
  local checksum="$1"
  shift
  local bin_dir="$1"
  shift
  local bin_in_archive=("$@") # path to bin in archive
  if [[ "${bin_dir}" == "${install_action_dir}/bin" ]]; then
    init_install_action_bin_dir
  fi

  installed_bin=()
  local tmp
  case "${tool}" in
    # xbuild's binary name is "x", as opposed to the usual crate name
    xbuild) installed_bin=("${bin_dir}/x${exe}") ;;
    # Their binary names are renamed below.
    editorconfig-checker | oxfmt) installed_bin=("${bin_dir}/${tool}${exe}") ;;
    *)
      for tmp in "${bin_in_archive[@]}"; do
        installed_bin+=("${bin_dir}/$(basename -- "${tmp}")")
      done
      ;;
  esac

  local tar_args=()
  case "${url}" in
    *.tar.gz | *.tgz)
      tar_args+=('xzf')
      if ! type -P gzip >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (gzip)\n'
            sys_install gzip
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
    *.gz)
      if ! type -P gzip >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (gzip)\n'
            sys_install gzip
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
    *.tar.bz2 | *.tbz2)
      tar_args+=('xjf')
      if ! type -P bzip2 >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (bzip2)\n'
            sys_install bzip2
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
    *.tar.xz | *.txz)
      tar_args+=('xJf')
      if ! type -P xz >/dev/null; then
        case "${base_distro}" in
          debian)
            printf '::group::Install packages required for installation (xz-utils)\n'
            sys_install xz-utils
            printf '::endgroup::\n'
            ;;
          fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (xz)\n'
            sys_install xz
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
    *.tar.zst | *.tzst)
      tar_args+=('xf')
      if ! type -P zstd >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (zstd)\n'
            sys_install zstd
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
    *.zip)
      if ! type -P unzip >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (unzip)\n'
            sys_install unzip
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
    *.deb)
      if ! type -P dpkg-deb >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (dpkg)\n'
            sys_install dpkg
            printf '::endgroup::\n'
            ;;
        esac
      fi
      ;;
  esac

  mkdir -p -- "${tmp_dir}"
  (
    cd -- "${tmp_dir}"
    download_and_checksum "${url}" "${checksum}"
    if [[ ${#tar_args[@]} -gt 0 ]]; then
      case "${url}" in
        *.tar.zst | *.tzst) zstd -dc tmp | tar "${tar_args[@]}" - ;;
        *) tar "${tar_args[@]}" tmp ;;
      esac
      for tmp in "${bin_in_archive[@]}"; do
        case "${tool}" in
          editorconfig-checker | oxfmt) mv -- "${tmp}" "${bin_dir}/${tool}${exe}" ;;
          *) mv -- "${tmp}" "${bin_dir}/" ;;
        esac
      done
    else
      case "${url}" in
        *.zip)
          unzip -q tmp
          for tmp in "${bin_in_archive[@]}"; do
            case "${tool}" in
              editorconfig-checker | oxfmt) mv -- "${tmp}" "${bin_dir}/${tool}${exe}" ;;
              *) mv -- "${tmp}" "${bin_dir}/" ;;
            esac
          done
          ;;
        *.gz)
          mv -- tmp "${bin_in_archive#\./}.gz"
          gzip -d "${bin_in_archive#\./}.gz"
          for tmp in "${bin_in_archive[@]}"; do
            mv -- "${tmp}" "${bin_dir}/"
          done
          ;;
        *.deb)
          dpkg-deb -x tmp .
          for tmp in "${bin_in_archive[@]}"; do
            mv -- "${tmp}" "${bin_dir}/"
          done
          ;;
        *)
          for tmp in "${installed_bin[@]}"; do
            mv -- tmp "${tmp}"
          done
          ;;
      esac
    fi
  )
  rm -rf -- "${tmp_dir}"

  case "${host_os}" in
    linux | macos)
      for tmp in "${installed_bin[@]}"; do
        if [[ ! -x "${tmp}" ]]; then
          chmod +x "${tmp}"
        fi
      done
      ;;
    windows)
      case "$(uname -s)" in
        CYGWIN* | MINGW* | MSYS*)
          for tmp in "${installed_bin[@]}"; do
            if [[ ! -x "${tmp}" ]]; then
              chmod +x "${tmp}"
            fi
          done
          ;;
      esac
      ;;
  esac
}
read_manifest() {
  local tool="$1"
  local version="$2"
  local manifest
  rust_crate=$(jq -r '.rust_crate' "${manifest_dir}/${tool}.json")
  manifest=$(jq -r --arg version "${version}" '.[$version]' "${manifest_dir}/${tool}.json")
  if [[ "${manifest}" == "null" ]]; then
    download_info="null"
    return 0
  fi
  exact_version=$(jq -r '.version' <<<"${manifest}")
  if [[ "${exact_version}" == "null" ]]; then
    exact_version="${version}"
  else
    manifest=$(jq -r --arg version "${exact_version}" '.[$version]' "${manifest_dir}/${tool}.json")
  fi

  case "${host_os}" in
    linux)
      # Static-linked binaries compiled for linux-musl will also work on linux-gnu systems and are
      # usually preferred over linux-gnu binaries because they can avoid glibc version issues.
      # (rustc enables statically linking for linux-musl by default, except for mips.)
      host_platform="${host_arch}_linux_musl"
      download_info=$(jq -r --arg p "${host_platform}" '.[$p]' <<<"${manifest}")
      if [[ "${download_info}" == "null" ]]; then
        # Even if host_env is musl, we won't issue an error here because it seems that in
        # some cases linux-gnu binaries will work on linux-musl hosts.
        # https://wiki.alpinelinux.org/wiki/Running_glibc_programs
        # TODO: However, a warning may make sense.
        host_platform="${host_arch}_linux_gnu"
        download_info=$(jq -r --arg p "${host_platform}" '.[$p]' <<<"${manifest}")
      elif [[ "${host_env}" == "gnu" ]]; then
        # TODO: don't hardcode tool name and use 'prefer_linux_gnu' field in base manifest.
        case "${tool}" in
          cargo-nextest)
            # TODO: don't hardcode required glibc version
            required_glibc_version=2.27
            higher_glibc_version=$(LC_ALL=C sort -Vu <<<"${required_glibc_version}"$'\n'"${host_glibc_version}" | tail -1)
            if [[ "${higher_glibc_version}" == "${host_glibc_version}" ]]; then
              # musl build of nextest is slow, so use glibc build if host_env is gnu.
              # https://github.com/taiki-e/install-action/issues/13
              host_platform="${host_arch}_linux_gnu"
              download_info=$(jq -r --arg p "${host_platform}" '.[$p]' <<<"${manifest}")
            fi
            ;;
        esac
      fi
      ;;
    macos | windows)
      # Binaries compiled for x86_64 macOS will usually also work on AArch64 macOS.
      # Binaries compiled for x86_64 Windows will usually also work on AArch64 Windows 11+.
      host_platform="${host_arch}_${host_os}"
      download_info=$(jq -r --arg p "${host_platform}" '.[$p]' <<<"${manifest}")
      if [[ "${download_info}" == "null" ]] && [[ "${host_arch}" != "x86_64" ]]; then
        host_platform="x86_64_${host_os}"
        download_info=$(jq -r --arg p "${host_platform}" '.[$p]' <<<"${manifest}")
      fi
      ;;
    *) bail "unsupported OS type '${host_os}' for ${tool}" ;;
  esac
}
read_download_info() {
  local tool="$1"
  local version="$2"
  if [[ "${download_info}" == "null" ]]; then
    bail "${tool}@${version} for '${host_os}' is not supported"
  fi
  checksum=$(jq -r '.hash' <<<"${download_info}")
  url=$(jq -r '.url' <<<"${download_info}")
  local tmp
  bin_in_archive=()
  if [[ "${url}" == "null" ]]; then
    local template
    template=$(jq -c --arg p "${host_platform}" '.template[$p]' "${manifest_dir}/${tool}.json")
    template="${template//\$\{version\}/${exact_version}}"
    url=$(jq -r '.url' <<<"${template}")
    tmp=$(jq -r '.bin' <<<"${template}")
    if [[ "${tmp}" == *"["* ]]; then
      # shellcheck disable=SC2207
      bin_in_archive=($(jq -r '.bin[]' <<<"${template}"))
    fi
  else
    tmp=$(jq -r '.bin' <<<"${download_info}")
    if [[ "${tmp}" == *"["* ]]; then
      # shellcheck disable=SC2207
      bin_in_archive=($(jq -r '.bin[]' <<<"${download_info}"))
    fi
  fi
  if [[ ${#bin_in_archive[@]} -eq 0 ]]; then
    if [[ "${tmp}" == "null" ]]; then
      bin_in_archive=("${tool}${exe}")
    else
      bin_in_archive=("${tmp}")
    fi
  fi
  if [[ "${rust_crate}" == "null" ]]; then
    # Moving files to /usr/local/bin requires sudo in some environments, so do not use it: https://github.com/taiki-e/install-action/issues/543
    bin_dir="${install_action_dir}/bin"
  else
    bin_dir="${cargo_bin}"
  fi
}
download_from_manifest() {
  read_manifest "$@"
  download_from_download_info "$@"
}
download_from_download_info() {
  read_download_info "$@"
  download_and_extract "${url}" "${checksum}" "${bin_dir}" "${bin_in_archive[@]}"
}
install_cargo_binstall() {
  local binstall_version
  binstall_version=$(jq -r '.latest.version' "${manifest_dir}/cargo-binstall.json")
  local install_binstall=1
  _binstall_version=$("cargo-binstall${exe}" binstall -V 2>/dev/null || true)
  if [[ -n "${_binstall_version}" ]]; then
    if [[ "${_binstall_version}" == "${binstall_version}" ]]; then
      info "cargo-binstall already installed at ${cargo_bin}/cargo-binstall${exe}"
      install_binstall=''
    else
      info "cargo-binstall already installed at ${cargo_bin}/cargo-binstall${exe}, but is not compatible version with this action, upgrading"
      rm -- "${cargo_bin}/cargo-binstall${exe}"
    fi
  fi

  if [[ -n "${install_binstall}" ]]; then
    info "installing cargo-binstall@latest (${binstall_version})"
    download_from_manifest "cargo-binstall" "latest"
    installed_at=$(type -P "cargo-binstall${exe}" || true)
    if [[ -n "${installed_at}" ]]; then
      info "cargo-binstall installed at ${installed_at}"
    else
      warn "cargo-binstall should be installed at ${bin_dir:-}/cargo-binstall${exe}; but cargo-binstall${exe} not found in path"
    fi
    rx "cargo-binstall${exe}" binstall -V
  fi
}
apt_update() {
  retry _sudo apt-get -o Acquire::Retries=10 -qq update
  apt_updated=1
}
apt_install() {
  if [[ -z "${apt_updated:-}" ]]; then
    apt_update
  fi
  retry _sudo apt-get -o Acquire::Retries=10 -o Dpkg::Use-Pty=0 install -y --no-install-recommends "$@"
}
apt_remove() {
  _sudo apt-get -qq -o Dpkg::Use-Pty=0 remove -y "$@"
}
snap_install() {
  retry _sudo snap install "$@"
}
dnf_install() {
  retry _sudo "${dnf}" install -y "$@"
}
zypper_install() {
  retry _sudo zypper install -y "$@"
}
pacman_install() {
  retry _sudo pacman -Sy --noconfirm "$@"
}
apk_install() {
  if type -P sudo >/dev/null; then
    retry sudo apk --no-cache add "$@"
  elif type -P doas >/dev/null; then
    retry doas apk --no-cache add "$@"
  else
    retry apk --no-cache add "$@"
  fi
}
sys_install() {
  case "${base_distro}" in
    debian) apt_install "$@" ;;
    fedora) dnf_install "$@" ;;
    suse) zypper_install "$@" ;;
    arch) pacman_install "$@" ;;
    alpine) apk_install "$@" ;;
  esac
}
init_install_action_bin_dir() {
  if [[ -z "${init_install_action_bin:-}" ]]; then
    init_install_action_bin=1
    mkdir -p -- "${bin_dir}"
    export PATH="${PATH}:${bin_dir}"
    local _bin_dir
    _bin_dir=$(canonicalize_windows_path "${bin_dir}")
    # TODO: avoid this when already added
    info "adding '${_bin_dir}' to PATH"
    printf '%s\n' "${_bin_dir}" >>"${GITHUB_PATH}"
  fi
}
canonicalize_windows_path() {
  case "${host_os}" in
    windows)
      local t="$1"
      if [[ "${t}" == '/cygdrive/'* ]]; then
        t="${t#/cygdrive}"
      fi
      if [[ "${t}" == '/c/'* ]]; then
        t="${t/\/c\//C:\\}"
      fi
      printf '%s\n' "${t//\//\\}"
      ;;
    *) printf '%s\n' "$1" ;;
  esac
}

if [[ $# -gt 0 ]]; then
  bail "invalid argument '$1'"
fi

# Inputs
tool="${INPUT_TOOL:-}"
tools=()
if [[ -n "${tool}" ]]; then
  while IFS= read -rd,; do
    tools+=("${REPLY}")
  done < <(normalize_comma_or_space_separated "${tool}")
fi
if [[ ${#tools[@]} -eq 0 ]]; then
  warn "no tool specified; this could be caused by a dependabot bug where @<tool_name> tags on this action are replaced by @<version> tags"
  # Exit with 0 for backward compatibility.
  # TODO: We want to reject it in the next major release.
  exit 0
fi

enable_checksum="${INPUT_CHECKSUM:-}"
case "${enable_checksum}" in
  true) ;;
  false)
    enable_checksum=''
    warn "checksums have been disabled by 'checksum' input option; this is strongly discouraged for security reasons"
    ;;
  *) bail "'checksum' input option must be 'true' or 'false': '${enable_checksum}'" ;;
esac

fallback="${INPUT_FALLBACK:-}"
case "${fallback}" in
  none | cargo-binstall | cargo-install) ;;
  *) bail "'fallback' input option must be 'none', 'cargo-binstall', or 'cargo-install': '${fallback}'" ;;
esac

# Unlike gh command, cargo-binstall reads GITHUB_TOKEN first via cli parser, and then reads GH_TOKEN.
# https://github.com/cargo-bins/cargo-binstall/blob/v1.17.9/crates/bin/src/args.rs#L704
token="${GITHUB_TOKEN:-"${GH_TOKEN:-"${DEFAULT_GITHUB_TOKEN:-}"}"}"
# This prevents tokens from being exposed to subprocesses via environment variables.
# Since the tokens remain in memory, setting `fallback: none` (which prevents the tokens from being
# set in the first place) remains the best practice from a security standpoint, as readme says.
# Note that this does not prevent token leaks via reading `/proc/*/environ` on Linux or
# via `ps -Eww` on macOS. It only reduces the risk of leaks.
unset GITHUB_TOKEN GH_TOKEN DEFAULT_GITHUB_TOKEN

# Refs:
# - https://github.com/rust-lang/rustup/blob/HEAD/rustup-init.sh
# - https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#runner-context
# NB: Sync with tools/ci/tool-list.sh.
base_distro=''
exe=''
case "${RUNNER_OS}" in
  Linux)
    host_os=linux
    ldd_version=$(ldd --version 2>&1 || true)
    if [[ "${ldd_version}" == *'musl'* ]]; then
      host_env=musl
    else
      host_env=gnu
      host_glibc_version=$(grep -E "GLIBC|GNU libc" <<<"${ldd_version}")
      host_glibc_version="${host_glibc_version##* }"
    fi
    if [[ -e /etc/redhat-release ]]; then
      # /etc/os-release is available on RHEL/CentOS 7+
      base_distro=fedora
    elif [[ -e /etc/debian_version ]]; then
      # /etc/os-release is available on Debian 7+
      base_distro=debian
    elif [[ -e /etc/os-release ]]; then
      if grep -Eq '^ID_LIKE=' /etc/os-release; then
        base_distro=$(grep -E '^ID_LIKE=' /etc/os-release | cut -d= -f2)
        case "${base_distro}" in
          *debian*) base_distro=debian ;;
          *fedora*) base_distro=fedora ;;
          *suse*) base_distro=suse ;;
          *arch*) base_distro=arch ;;
          *alpine*) base_distro=alpine ;;
        esac
      else
        base_distro=$(grep -E '^ID=' /etc/os-release | cut -d= -f2)
      fi
      base_distro="${base_distro//\"/}"
    fi
    case "${base_distro}" in
      fedora)
        dnf=dnf
        if ! type -P dnf >/dev/null; then
          if type -P microdnf >/dev/null; then
            # fedora-based distributions have "minimal" images that
            # use microdnf instead of dnf.
            dnf=microdnf
          else
            # If neither dnf nor microdnf is available, it is
            # probably an RHEL7-based distribution that does not
            # have dnf installed by default.
            dnf=yum
          fi
        fi
        ;;
    esac
    ;;
  macOS) host_os=macos ;;
  Windows)
    host_os=windows
    exe=.exe
    ;;
  *) bail "unrecognized runner.os '${RUNNER_OS}'" ;;
esac
case "${RUNNER_ARCH}" in
  X64) host_arch=x86_64 ;;
  # Always fallback to `cargo install` on 32-bit Arm for now, as we need to
  # consider the version and whether hard-float is supported.
  # https://github.com/rust-lang/rustup/pull/593
  # https://github.com/cross-rs/cross/pull/1018
  # And support for 32-bit Arm will be removed in near future.
  # https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/#removal-of-operating-system-support-with-node24
  # Does it seem only armv7l+ is supported?
  # https://github.com/actions/runner/blob/v2.321.0/src/Misc/externals.sh#L178
  # https://github.com/actions/runner/issues/688
  ARM) host_arch=arm ;;
  X86) host_arch=x86 ;;
  ARM64) host_arch=aarch64 ;;
  PPC64LE) host_arch=powerpc64le ;;
  RISCV64) host_arch=riscv64 ;;
  S390X) host_arch=s390x ;;
  *)
    info "unrecognized runner.arch '${RUNNER_ARCH}'; fallback to uname -m"
    host_arch="$(uname -m)"
    case "${host_arch}" in
      aarch64 | arm64) host_arch=aarch64 ;;
      ppc64le) host_arch=powerpc64le ;;
      riscv64) host_arch=riscv64 ;;
      s390x) host_arch=s390x ;;
      # On these platforms, we just use the result of `uname -m` as host_arch, and always fallback to `cargo install`.
      xscale | arm | armv*l | loongarch64 | mips | mips64 | ppc | ppc64 | sun4v) ;;
      # GitHub Actions Runner supports x86_64/AArch64/Arm Linux and x86_64/AArch64 Windows/macOS.
      # https://github.com/actions/runner/blob/v2.332.0/.github/workflows/build.yml#L24
      # https://docs.github.com/en/actions/reference/runners/self-hosted-runners#supported-processor-architectures
      # And IBM provides runners for powerpc64le/s390x Linux.
      # https://github.com/IBM/actionspz
      # So we can assume x86_64 unless it has a known non-x86_64 uname -m result.
      *) host_arch=x86_64 ;;
    esac
    ;;
esac
info "host platform: ${host_arch}_${host_os}"

home="${HOME:-}"
if [[ -z "${home}" ]]; then
  # https://github.com/IBM/actionspz/issues/30
  home=$(realpath ~)
  export HOME="${home}"
fi
if [[ "${host_os}" == "windows" ]]; then
  if [[ "${home}" == "/home/"* ]]; then
    if [[ -d "${home/\/home\///c/Users/}" ]]; then
      # MSYS2 https://github.com/taiki-e/install-action/pull/518#issuecomment-2160736760
      home="${home/\/home\///c/Users/}"
    elif [[ -d "${home/\/home\///cygdrive/c/Users/}" ]]; then
      # Cygwin https://github.com/taiki-e/install-action/issues/224#issuecomment-1720196288
      home="${home/\/home\///cygdrive/c/Users/}"
    else
      warn "\$HOME starting /home/ (${home}) on Windows bash is usually fake path, this may cause installation issue"
    fi
  fi
fi
install_action_dir="${home}/.install-action"
tmp_dir="${install_action_dir}/tmp"
cargo_bin="${CARGO_HOME:-"${home}/.cargo"}/bin"
cargo_path=$(type -P cargo || true)
# If $CARGO_HOME does not exist, or cargo installed outside of $CARGO_HOME/bin
# is used ($CARGO_HOME/bin is most likely not included in the PATH), fallback to
# $install_action_dir/bin.
if [[ "${host_os}" == "windows" ]]; then
  mkdir -p -- "${install_action_dir}"
  # See action.yml.
  printf '' >|"${install_action_dir}"/init
  if [[ -n "${cargo_path}" ]]; then
    info "cargo is located at ${cargo_path}"
    cargo_bin="${cargo_path%/*}"
  else
    cargo_bin="${install_action_dir}/bin"
  fi
elif [[ ! -e "${cargo_bin}" ]] || [[ "${cargo_path}" != "${cargo_bin}/cargo"* ]]; then
  if [[ -n "${cargo_path}" ]]; then
    info "cargo is located at ${cargo_path}"
  fi
  # Moving files to /usr/local/bin requires sudo in some environments, so do not use it: https://github.com/taiki-e/install-action/issues/543
  cargo_bin="${install_action_dir}/bin"
fi

# cargo-binstall may call `cargo install` on their fallback: https://github.com/taiki-e/install-action/pull/54#issuecomment-1383140833
# cross calls rustup on `cross --version` if the current directly is cargo workspace.
export CARGO_NET_RETRY=10
export RUSTUP_MAX_RETRIES=10

export DEBIAN_FRONTEND=noninteractive
manifest_dir="${GITHUB_ACTION_PATH}/manifests"

case "${host_os}" in
  linux)
    if ! type -P jq >/dev/null || ! type -P curl >/dev/null || ! type -P tar >/dev/null; then
      case "${base_distro}" in
        debian | fedora | suse | arch | alpine)
          printf '::group::Install packages required for installation (jq, curl, and/or tar)\n'
          sys_packages=()
          if ! type -P curl >/dev/null; then
            sys_packages+=(ca-certificates curl)
          fi
          if ! type -P tar >/dev/null; then
            sys_packages+=(tar)
          fi
          if [[ "${dnf:-}" == "yum" ]]; then
            # On RHEL7-based distribution jq requires EPEL
            if ! type -P jq >/dev/null; then
              sys_packages+=(epel-release)
              sys_install "${sys_packages[@]}"
              sys_install jq --enablerepo=epel
            else
              sys_install "${sys_packages[@]}"
            fi
          else
            if ! type -P jq >/dev/null; then
              # https://github.com/taiki-e/install-action/issues/521
              if [[ "${base_distro}" == "arch" ]]; then
                sys_packages+=(glibc)
              fi
              sys_packages+=(jq)
            fi
            sys_install "${sys_packages[@]}"
          fi
          printf '::endgroup::\n'
          ;;
        *) warn "this action requires at least jq and curl on non-Debian/Fedora/SUSE/Arch/Alpine-based Linux" ;;
      esac
    fi
    ;;
  macos)
    if ! type -P jq >/dev/null || ! type -P curl >/dev/null; then
      warn "this action requires at least jq and curl on macOS"
    fi
    ;;
  windows)
    if ! type -P curl >/dev/null; then
      warn "this action requires at least curl on Windows"
    fi
    if [[ -f "${install_action_dir}/jq/bin/jq.exe" ]]; then
      jq() { "${install_action_dir}/jq/bin/jq.exe" -b "$@"; }
    elif type -P jq >/dev/null; then
      # https://github.com/jqlang/jq/issues/1854
      _tmp=$(jq -r .a <<<'{}' | wc -c)
      if [[ "${_tmp}" != 5 ]]; then
        _tmp=$({ jq -b -r .a 2>/dev/null <<<'{}' || true; } | wc -c)
        if [[ "${_tmp}" == 5 ]]; then
          jq() { command jq -b "$@"; }
        else
          jq() { command jq "$@" | tr -d '\r'; }
        fi
      fi
    else
      printf '::group::Install packages required for installation (jq)\n'
      mkdir -p -- "${install_action_dir}/jq/bin"
      url='https://github.com/jqlang/jq/releases/download/jq-1.8.1/jq-windows-amd64.exe'
      checksum='23cb60a1354eed6bcc8d9b9735e8c7b388cd1fdcb75726b93bc299ef22dd9334'
      (
        cd -- "${install_action_dir}/jq/bin"
        download_and_checksum "${url}" "${checksum}"
        mv -- tmp jq.exe
      )
      printf '::endgroup::\n'
      jq() { "${install_action_dir}/jq/bin/jq.exe" -b "$@"; }
    fi
    ;;
  *) bail "unsupported host OS '${host_os}'" ;;
esac

unsupported_tools=()
for tool in "${tools[@]}"; do
  additional=''
  if [[ "${tool}" == *'+'* ]]; then
    additional="${tool#*+}"
    tool="${tool%%+*}"
  fi
  if [[ "${tool}" == *'@'* ]]; then
    version="${tool#*@}"
    tool="${tool%@*}"
    if [[ "${tool}" != 'rust' ]]; then
      if [[ ! "${version}" =~ ^([1-9][0-9]*(\.[0-9]+(\.[0-9]+)?)?|0\.[1-9][0-9]*(\.[0-9]+)?|^0\.0\.[0-9]+)(-[0-9A-Za-z\.-]+)?$|^latest$ ]]; then
        if [[ ! "${version}" =~ ^([1-9][0-9]*(\.[0-9]+(\.[0-9]+)?)?|0\.[1-9][0-9]*(\.[0-9]+)?|^0\.0\.[0-9]+)(-[0-9A-Za-z\.-]+)?(\+[0-9A-Za-z\.-]+)?$|^latest$ ]]; then
          bail "semver operators are not supported in 'tool' input option: '${version}'"
        fi
        bail "install-action v2 does not support semver build-metadata: '${version}'; if you need these supports again, please submit an issue at <https://github.com/taiki-e/install-action>"
      fi
    fi
  else
    version=latest
  fi
  if [[ -n "${additional}" ]]; then
    case "${tool}" in
      rust) ;;
      *) bail "<tool_name>+<additional> syntax is not supported for ${tool}" ;;
    esac
  fi
  installed_bin=()
  # NB: Update tools/ci/release-post-commit.sh when adding non-manifest-based tools.
  case "${tool}" in
    rust)
      if [[ "${version}" == 'latest' ]]; then
        version=stable
      fi
      info "installing ${tool}@${version}"
      export RUSTUP_MAX_RETRIES="${RUSTUP_MAX_RETRIES:-10}"
      rustup_args=(--profile minimal)
      if [[ -n "${additional}" ]]; then
        component=''
        target=''
        while IFS= read -rd+; do
          case "${REPLY}" in
            # Last checked: nightly-2026-05-03
            # rustup component list
            # rustup target list
            cargo | cargo-* | clippy | clippy-* | llvm-* | miri | miri-* | rust-* | rustc-* | rustfmt | rustfmt-*) component+=",${REPLY}" ;;
            *) target+=",${REPLY}" ;;
          esac
        done <<<"${additional}+"
        if [[ -n "${component}" ]]; then
          if [[ "${component}," == *',miri,'* ]] && [[ "${component}," != *',rust-src,'* ]]; then
            component+=',rust-src'
          fi
          rustup_args+=(--component "${component#,}")
        fi
        if [[ -n "${target}" ]]; then
          rustup_args+=(--target "${target#,}")
        fi
      fi
      if type -P rustup >/dev/null; then
        # --no-self-update is necessary because the windows environment cannot self-update rustup.exe.
        g retry rustup toolchain add "${version}" --no-self-update "${rustup_args[@]}"
        g rustup default "${version}"
      else
        # https://github.com/rust-lang/rustup/tags
        # Run tools/rustup-hash.sh to get sha256 hash.
        rustup_version=1.29.0
        # https://rust-lang.github.io/rustup/installation/other.html#manual-installation
        rust_target=''
        checksum=''
        case "${host_os}" in
          linux)
            rust_target="${host_arch}-unknown-${host_os}-${host_env}"
            case "${host_arch}" in
              x86_64)
                case "${host_env}" in
                  gnu) checksum=4acc9acc76d5079515b46346a485974457b5a79893cfb01112423c89aeb5aa10 ;;
                  musl) checksum=9cd3fda5fd293890e36ab271af6a786ee22084b5f6c2b83fd8323cec6f0992c1 ;;
                esac
                ;;
              aarch64)
                case "${host_env}" in
                  gnu) checksum=9732d6c5e2a098d3521fca8145d826ae0aaa067ef2385ead08e6feac88fa5792 ;;
                  musl) checksum=88761caacddb92cd79b0b1f939f3990ba1997d701a38b3e8dd6746a562f2a759 ;;
                esac
                ;;
              powerpc64le)
                case "${host_env}" in
                  gnu) checksum=4bfff85bd3967d988e14567aa9cc6ab0ea386f0ffeff0f9f14d23f0103bf1f97 ;;
                  musl) checksum=e15d033af90b7a55d170aac2d82cc28ddd96dbfcdda7c6d4eb8cb064a99c4646 ;;
                esac
                ;;
              riscv64)
                rust_target="${host_arch}gc-unknown-${host_os}-${host_env}"
                # riscv64gc-unknown-linux-musl is tier 2 without host tools
                case "${host_env}" in
                  gnu) checksum=7e43f2b2e6307d61da17a4dff61e6bceef408b8189822df64e1094590d2a70f9 ;;
                esac
                ;;
              s390x)
                # s390x-unknown-linux-musl is tier 3
                case "${host_env}" in
                  gnu) checksum=66c2c132428b6b77803facb02cbdf33b89d20c00bd20da142be8cb651f2e7cd8 ;;
                esac
                ;;
            esac
            ;;
          macos)
            rust_target="${host_arch}-apple-darwin"
            case "${host_arch}" in
              x86_64) checksum=33cf85df9142bc6d29cbc62fa5ca1d4c29622cddb55213a4c1a43c457fb9b2d7 ;;
              aarch64) checksum=aeb4105778ca1bd3c6b0e75768f581c656633cd51368fa61289b6a71696ac7e1 ;;
            esac
            ;;
          windows)
            rust_target="${host_arch}-pc-windows-msvc"
            case "${host_arch}" in
              x86_64) checksum=86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7 ;;
              aarch64) checksum=3af309e6c3062aa11df0e932954f69d13b734d8a431e593812f3ecd9ff9e6ef6 ;;
            esac
            ;;
        esac
        if [[ -z "${rust_target}" ]] || [[ -z "${checksum}" ]]; then
          bail "unsupported host platform ${host_arch}_${host_os} for ${tool}"
        fi
        url="https://static.rust-lang.org/rustup/archive/${rustup_version}/${rust_target}/rustup-init${exe}"
        mkdir -p -- "${tmp_dir}"
        (
          cd -- "${tmp_dir}"
          download_and_checksum "${url}" "${checksum}"
          mv -- tmp rustup-init
          case "${host_os}" in
            linux | macos) chmod +x ./rustup-init ;;
          esac
          g retry ./rustup-init -y --default-toolchain "${version}" --no-modify-path "${rustup_args[@]}"
        )
        rm -rf -- "${tmp_dir}"
        cargo_bin_dir="${CARGO_HOME:-"${home}/.cargo"}/bin"
        export PATH="${PATH}:${cargo_bin_dir}"
        cargo_bin_dir=$(canonicalize_windows_path "${cargo_bin_dir}")
        info "adding '${cargo_bin_dir}' to PATH"
        printf '%s\n' "${cargo_bin_dir}" >>"${GITHUB_PATH}"
        cargo_path=$(type -P cargo || true)
      fi
      installed_bin=("rustc${exe}" "cargo${exe}")
      ;;
    protoc)
      info "installing ${tool}@${version}"
      read_manifest "protoc" "${version}"
      read_download_info "protoc" "${version}"
      # Copying files to /usr/local/include requires sudo, so do not use it.
      bin_dir="${install_action_dir}/bin"
      include_dir="${install_action_dir}/include"
      init_install_action_bin_dir
      if [[ ! -e "${include_dir}" ]]; then
        mkdir -p -- "${include_dir}"
      fi
      if ! type -P unzip >/dev/null; then
        case "${base_distro}" in
          debian | fedora | suse | arch | alpine)
            printf '::group::Install packages required for installation (unzip)\n'
            sys_install unzip
            printf '::endgroup::\n'
            ;;
        esac
      fi
      mkdir -p -- "${tmp_dir}"
      (
        cd -- "${tmp_dir}"
        download_and_checksum "${url}" "${checksum}"
        unzip -q tmp
        mv -- "bin/protoc${exe}" "${bin_dir}/"
        mkdir -p -- "${include_dir}/"
        cp -r -- include/. "${include_dir}/"
        if [[ -z "${PROTOC:-}" ]]; then
          _bin_dir=$(canonicalize_windows_path "${bin_dir}")
          info "setting PROTOC environment variable to '${_bin_dir}/protoc${exe}'"
          printf '%s\n' "PROTOC=${_bin_dir}/protoc${exe}" >>"${GITHUB_ENV}"
        fi
      )
      rm -rf -- "${tmp_dir}"
      installed_bin=("${tool}${exe}")
      ;;
    valgrind)
      info "installing ${tool}@${version}"
      case "${version}" in
        latest) ;;
        *) warn "specifying the version of ${tool} is not supported" ;;
      esac
      case "${host_os}" in
        linux) ;;
        macos | windows) bail "${tool} for non-Linux is not supported" ;;
        *) bail "unsupported host OS '${host_os}' for ${tool}" ;;
      esac
      # libc6-dbg is needed to run Valgrind
      apt_install libc6-dbg
      # Use snap to install the latest Valgrind
      # https://snapcraft.io/install/valgrind/ubuntu
      snap_install valgrind --classic
      installed_bin=("${tool}${exe}")
      ;;
    cargo-binstall)
      case "${version}" in
        latest) ;;
        *) warn "specifying the version of ${tool} is not supported" ;;
      esac
      install_cargo_binstall
      printf '\n'
      continue
      ;;
    *)
      # Handle aliases.
      # NB: Update alias list in tools/ci/release-post-commit.sh, tool input option in test-alias in .github/workflows/ci.yml,
      # and match for alias for tools/codegen/src/tools-markdown.rs.
      # TODO(codegen): auto-detect cases where crate name and tool name are different.
      case "${tool}" in
        nextest) tool=cargo-nextest ;;
        wild-linker) tool=wild ;;
        taplo-cli | typos-cli | wasm-bindgen-cli | wasmtime-cli) tool="${tool%-cli}" ;;
      esac

      # Use cargo-binstall fallback if tool is not available.
      if [[ ! -f "${manifest_dir}/${tool}.json" ]]; then
        case "${version}" in
          latest) unsupported_tools+=("${tool}") ;;
          *) unsupported_tools+=("${tool}@${version}") ;;
        esac
        continue
      fi

      # Use cargo-binstall fallback if tool is available but the specified version not available.
      read_manifest "${tool}" "${version}"
      if [[ "${download_info}" == "null" ]]; then
        if [[ "${rust_crate}" == "null" ]] || [[ "${fallback}" == "none" ]]; then
          bail "${tool} is supported but version ${version} for '${host_arch}_${host_os}' is not supported (updating install-action might resolve this)"
        fi
        warn "${tool} is supported but version ${version} for '${host_arch}_${host_os}' is not supported (updating install-action might resolve this); fallback to ${fallback}"
        case "${version}" in
          latest) unsupported_tools+=("${rust_crate}") ;;
          *) unsupported_tools+=("${rust_crate}@${version}") ;;
        esac
        continue
      fi

      info "installing ${tool}@${version}"

      # Pre-install
      case "${tool}" in
        shellcheck)
          case "${host_os}" in
            linux)
              if type -P shellcheck >/dev/null; then
                apt_remove -y shellcheck
              fi
              ;;
          esac
          ;;
        cyclonedx)
          case "${host_os}" in
            linux)
              apt_install libicu-dev
              ;;
          esac
          ;;
      esac

      download_from_download_info "${tool}" "${version}"
      ;;
  esac

  tool_bin_stems=()
  for tool_bin in "${installed_bin[@]}"; do
    tool_bin=$(basename -- "${tool_bin}")
    tool_bin_stem="${tool_bin%.exe}"
    installed_at=$(type -P "${tool_bin}" || true)
    if [[ -z "${installed_at}" ]]; then
      tool_bin="${tool_bin_stem}"
      installed_at=$(type -P "${tool_bin}" || true)
    fi
    if [[ -n "${installed_at}" ]]; then
      info "${tool_bin_stem} installed at ${installed_at}"
    else
      warn "${tool_bin_stem} should be installed at ${bin_dir:+"${bin_dir}/"}${tool_bin}${exe}; but ${tool_bin}${exe} not found in path"
    fi
    tool_bin_stems+=("${tool_bin_stem}")
  done
  for tool_bin_stem in "${tool_bin_stems[@]}"; do
    # cargo-udeps 0.1.30 and wasm-pack 0.12.0 do not support --version flag.
    case "${tool_bin_stem}" in
      # biome up to 1.2.2 exits with 1 on both --version and --help flags.
      # cargo-machete up to 0.6.0 does not support --version flag.
      # wait-for-them up to 0.4.0 does not support --version flag.
      # gungraun-runner up to 0.17.1 (exclusive) does not support --version flag.
      biome | cargo-machete | wait-for-them | gungraun-runner) rx "${tool_bin_stem}" --version || true ;;
      # these packages support neither --version nor --help flag.
      cargo-auditable | cargo-careful | wasm-bindgen-test-runner | cargo-apple-runner) ;;
      # wasm2es6js does not support --version flag and --help flag doesn't contains version info.
      wasm2es6js) ;;
      # iai-callgrind-runner --version works only with iai-callgrind in nearby Cargo.toml.
      iai-callgrind-runner) ;;
      # cargo-zigbuild/cargo-insta has no --version flag on `cargo $tool_bin_stem` subcommand.
      cargo-zigbuild | cargo-insta) rx "${tool_bin_stem}" --version ;;
      # these packages have version command instead of --version flag.
      cosign | deepsource | vacuum) rx "${tool_bin_stem}" version ;;
      cargo-*)
        case "${tool_bin_stem}" in
          # cargo-valgrind 2.1.0's --version flag just calls cargo's --version flag
          cargo-valgrind) rx "${tool_bin_stem}" "${tool_bin_stem#cargo-}" --help ;;
          *)
            if ! rx "${tool_bin_stem}" "${tool_bin_stem#cargo-}" --version; then
              rx "${tool_bin_stem}" "${tool_bin_stem#cargo-}" --help
            fi
            ;;
        esac
        ;;
      *)
        if ! rx "${tool_bin_stem}" --version; then
          rx "${tool_bin_stem}" --help
        fi
        ;;
    esac
  done
  printf '\n'
done

if [[ ${#unsupported_tools[@]} -gt 0 ]]; then
  IFS=','
  case "${fallback}" in
    none) bail "install-action does not support ${unsupported_tools[*]} (updating install-action might resolve this); aborting because fallback is disabled by 'fallback: none' input option" ;;
    cargo-binstall)
      case "${host_arch}" in
        x86_64 | aarch64 | riscv64) ;;
        *)
          info "cargo-binstall does not provide prebuilt binaries for this platform (${host_arch}); use 'cargo-install' fallback instead"
          fallback=cargo-install
          ;;
      esac
      ;;
  esac
  info "install-action does not support ${unsupported_tools[*]} (updating install-action might resolve this); fallback to ${fallback}"
  IFS=$'\n\t'
  case "${fallback}" in
    cargo-binstall)
      install_cargo_binstall
      # By default, cargo-binstall enforce downloads over secure transports only.
      # As a result, http will be disabled, and it will also set min tls version
      # to be 1.2
      binstall_args=(
        --force
        --no-confirm
        --locked
      )
      if [[ -n "${token}" ]]; then
        GITHUB_TOKEN="${token}" \
          cargo-binstall binstall "${binstall_args[@]}" "${unsupported_tools[@]}"
      else
        cargo-binstall binstall "${binstall_args[@]}" "${unsupported_tools[@]}"
      fi
      if [[ -z "${cargo_path}" ]]; then
        _bin_dir=$(canonicalize_windows_path "${home}/.cargo/bin")
        info "adding '${_bin_dir}' to PATH"
        printf '%s\n' "${_bin_dir}" >>"${GITHUB_PATH}"
      fi
      ;;
    cargo-install)
      cargo install --locked "${unsupported_tools[@]}"
      ;;
    *) bail "unhandled fallback ${fallback}" ;;
  esac
fi
