#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0 OR MIT
set -CeEuo pipefail
IFS=$'\n\t'
trap -- 's=$?; printf >&2 "%s\n" "${0##*/}:${LINENO}: \`${BASH_COMMAND}\` exit with ${s}"; exit ${s}' ERR
cd -- "$(dirname -- "$0")"/..

# Get sha256 hash of rustup-init binaries

# NB: Synch with main.sh.
rustup_version=1.29.0
targets=(
  x86_64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-gnu
  aarch64-unknown-linux-musl
  powerpc64le-unknown-linux-gnu
  powerpc64le-unknown-linux-musl
  riscv64gc-unknown-linux-gnu
  # riscv64gc-unknown-linux-musl # tier 2 without host tools: TODO: https://github.com/rust-lang/rust/issues/156191
  s390x-unknown-linux-gnu
  # s390x-unknown-linux-musl # tier 3
  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-pc-windows-msvc
  aarch64-pc-windows-msvc
)

for rust_target in "${targets[@]}"; do
  exe=''
  case "${rust_target}" in
    *-windows*) exe=.exe ;;
  esac
  url="https://static.rust-lang.org/rustup/archive/${rustup_version}/${rust_target}/rustup-init${exe}.sha256"
  printf '%s: ' "${rust_target}"
  curl --proto '=https' --tlsv1.2 -fsSL --retry 10 "${url}" | cut -d' ' -f1
done
