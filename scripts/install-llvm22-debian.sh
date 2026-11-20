#!/bin/sh
set -eu

# Both Bookworm images need matching LLVM libraries and clang for native links.
curl --proto '=https' --tlsv1.2 -fsSL --retry 3 \
  https://apt.llvm.org/llvm-snapshot.gpg.key -o /usr/share/keyrings/llvm.asc
printf '%s\n' 'deb [signed-by=/usr/share/keyrings/llvm.asc] https://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-22 main' \
  > /etc/apt/sources.list.d/llvm22.list
apt-get update -o Acquire::Retries=3
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  llvm-22-dev llvm-22-tools libpolly-22-dev libzstd-dev clang-22
rm -rf /var/lib/apt/lists/*
/usr/lib/llvm-22/bin/llvm-config --version | grep -q '^22\.'
/usr/lib/llvm-22/bin/clang --version | grep -q 'version 22\.'
