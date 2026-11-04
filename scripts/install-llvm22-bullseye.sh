#!/bin/bash
# Install LLVM 22 packages from apt.llvm.org inside the bullseye image builds
# (scripts/linux-glibc-2.31.Dockerfile, scripts/linux-musl-llvm22.Dockerfile).
#
# usage: install-llvm22-bullseye.sh <key file> <package>...
#
# The signing key is COPYed from the build context (the file vendored for the
# setup-llvm22 action) instead of curled: a runner whose resolver cannot find
# apt.llvm.org (2026-11-03, run 41649955891: `curl: (6) Could not resolve host:
# apt.llvm.org`) then still gets past the key. The packages themselves need
# the mirror, so `apt-get update`/`install` are retried and, if they still
# fail, the failure names the resolver state.
set -euo pipefail

KEY_FPR=6084F3CF814B57C1CF12EFD515CF4D18AF4F7421
key_file=$1
shift

# apt trusts EVERY key in the file: it must hold exactly the pinned primary key.
got=$(gpg --show-keys --with-colons "$key_file" 2>/dev/null \
  | awk -F: '$1 == "pub" { p = 1; next } $1 == "fpr" && p { print $10; p = 0 }') || got=''
if [ "$got" != "$KEY_FPR" ]; then
  echo "error: $key_file holds key '$got', expected $KEY_FPR" >&2
  exit 1
fi
install -m 0644 "$key_file" /etc/apt/trusted.gpg.d/apt.llvm.org.asc
printf '%s\n' 'deb https://apt.llvm.org/bullseye/ llvm-toolchain-bullseye-22 main' \
  > /etc/apt/sources.list.d/llvm22.list

fail() {
  echo "error: $1 failed after 5 attempts; apt.llvm.org resolver state:" >&2
  getent hosts apt.llvm.org >&2 || echo "  getent hosts apt.llvm.org: no answer (DNS failure inside the build)" >&2
  exit 1
}

retry() {
  local what=$1 attempt
  shift
  for attempt in 1 2 3 4 5; do
    if "$@"; then return 0; fi
    echo "$what attempt $attempt failed; retrying" >&2
    sleep $((attempt * 10))
  done
  fail "$what"
}

retry "apt-get update" apt-get -o Acquire::Retries=5 update
retry "apt-get install" env DEBIAN_FRONTEND=noninteractive \
  apt-get -o Acquire::Retries=5 install -y --no-install-recommends "$@"
