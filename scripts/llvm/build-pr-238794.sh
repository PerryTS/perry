#!/usr/bin/env bash
# Build the LLVM 22.1.8 AArch64 memcmp-equality backport for Perry experiments.
# This never changes Perry's default LLVM dependency or the upstream source tree.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work_root="${PERRY_LLVM_EXPERIMENT_DIR:-${TMPDIR:-/tmp}/perry-llvm-pr-238794}"
source_dir="${work_root}/llvm-project"
build_dir="${work_root}/build"
prefix_dir="${work_root}/install"
jobs="${PERRY_LLVM_BUILD_JOBS:-8}"

mkdir -p "${work_root}"
if [[ -e "${source_dir}" && ! -d "${source_dir}/.git" ]]; then
  # Keep an incomplete or user-created directory untouched. Recover by cloning
  # into a fresh sibling instead of deleting or replacing its contents.
  source_dir="${work_root}/llvm-project-recovered-$(date +%Y%m%d%H%M%S)-$$"
fi

if [[ ! -e "${source_dir}" ]]; then
  git clone --depth 1 --branch llvmorg-22.1.8 \
    https://github.com/llvm/llvm-project.git "${source_dir}" || {
      echo "LLVM clone did not complete; its directory was preserved at ${source_dir}." >&2
      exit 1
    }
fi

if ! git -C "${source_dir}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  # A failed clone may leave an invalid .git directory. Preserve it and
  # recover into another fresh sibling rather than removing it.
  source_dir="${work_root}/llvm-project-recovered-$(date +%Y%m%d%H%M%S)-$$"
  git clone --depth 1 --branch llvmorg-22.1.8 \
    https://github.com/llvm/llvm-project.git "${source_dir}" || {
      echo "LLVM clone did not complete; its directory was preserved at ${source_dir}." >&2
      exit 1
    }
fi
remote_url="$(git -C "${source_dir}" remote get-url origin 2>/dev/null || true)"
if [[ "${remote_url}" != *"llvm/llvm-project"* ]]; then
  echo "Expected an llvm-project clone at ${source_dir}; found origin '${remote_url}'." >&2
  exit 1
fi
if ! git -C "${source_dir}" rev-parse --verify --quiet refs/tags/llvmorg-22.1.8 >/dev/null; then
  git -C "${source_dir}" fetch --depth 1 origin tag llvmorg-22.1.8
fi

patch_file="${repo_root}/patches/llvm/pr-238794-llvm-22.1.8.patch"
if git -C "${source_dir}" apply --unidiff-zero --reverse --check "${patch_file}" >/dev/null 2>&1; then
  git -C "${source_dir}" rev-parse HEAD | grep -qx "$(git -C "${source_dir}" rev-parse refs/tags/llvmorg-22.1.8)" || {
    echo "The patched LLVM checkout is not based on llvmorg-22.1.8: ${source_dir}" >&2
    exit 1
  }
  diff_without_index() {
    git -C "${source_dir}" diff --no-color --no-ext-diff --no-renames \
      --no-indent-heuristic --diff-algorithm=myers --unified=0 \
      --src-prefix=a/ --dst-prefix=b/ | sed '/^index /d'
  }
  if ! diff_without_index | cmp -s - <(sed '/^index /d' "${patch_file}") \
    || [[ -n "$(git -C "${source_dir}" ls-files --others --exclude-standard)" ]] \
    || [[ -n "$(git -C "${source_dir}" ls-files --others --ignored --exclude-standard)" ]]; then
    echo "The LLVM checkout contains changes beyond this patch: ${source_dir}" >&2
    exit 1
  fi
else
  if [[ -n "$(git -C "${source_dir}" status --porcelain --untracked-files=all --ignored)" ]]; then
    echo "Refusing to change a non-clean LLVM checkout: ${source_dir}" >&2
    echo "Choose a fresh PERRY_LLVM_EXPERIMENT_DIR to preserve existing work." >&2
    exit 1
  fi
  git -C "${source_dir}" checkout --detach llvmorg-22.1.8
  git -C "${source_dir}" apply --unidiff-zero --check "${patch_file}"
  git -C "${source_dir}" apply --unidiff-zero "${patch_file}"
fi

cmake -S "${source_dir}/llvm" -B "${build_dir}" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DLLVM_ENABLE_PROJECTS='' \
  -DLLVM_TARGETS_TO_BUILD='AArch64;X86' \
  -DLLVM_BUILD_TOOLS=ON \
  -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_ENABLE_TERMINFO=OFF \
  -DLLVM_ENABLE_ZLIB=OFF \
  -DLLVM_ENABLE_ZSTD=OFF \
  -DLLVM_ENABLE_LIBEDIT=OFF \
  -DLLVM_BUILD_LLVM_DYLIB=ON \
  -DLLVM_LINK_LLVM_DYLIB=ON \
  -DCMAKE_INSTALL_PREFIX="${prefix_dir}"

ninja -C "${build_dir}" -j "${jobs}" LLVM llvm-config \
  install-llvm-headers install-llvm-libraries install-llvm-config

echo "Patched LLVM prefix: ${prefix_dir}"
echo "Build Perry with: LLVM_SYS_221_PREFIX='${prefix_dir}' cargo build --profile perry-dev -p perry"
