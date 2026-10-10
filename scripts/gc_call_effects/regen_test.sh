#!/usr/bin/env bash
# Archive-path regression only: no Rust compilation or real target directory.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/gc-effects-regen-test.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
repo="$fixture/repo with spaces"
mkdir -p "$repo/scripts/gc_call_effects" "$fixture/bin"
cp "$HERE/regen.sh" "$repo/scripts/gc_call_effects/regen.sh"

cat > "$fixture/bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$FAKE_CARGO_LOG"
target_dir="${CARGO_TARGET_DIR:-target}"
if [[ "$1" == xwin ]]; then
  libdir="$target_dir/x86_64-pc-windows-msvc/release"
  libs=(perry_runtime.lib perry_stdlib.lib)
else
  libdir="$target_dir/release"
  libs=(libperry_runtime.a libperry_stdlib.a)
fi
mkdir -p "$libdir"
for lib in "${libs[@]}"; do : > "$libdir/$lib"; done
SH
cat > "$fixture/bin/python3" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$FAKE_PYTHON_LOG"
args=("$@")
for ((i=${#args[@]}-2; i<${#args[@]}; i++)); do
  [[ -f "${args[i]}" ]] || { echo "missing fake archive: ${args[i]}" >&2; exit 1; }
done
SH
chmod +x "$fixture/bin/cargo" "$fixture/bin/python3"

run_case() (
  local target="$1" target_dir="$2" expected_dir="$3" lib_override="${4:-}"
  unset CARGO_TARGET_DIR GC_EFFECTS_LIB_DIR GC_EFFECTS_SKIP_BUILD
  unset GC_EFFECTS_FRESH_OUT GC_EFFECTS_ALLOW_SAFE_DRIFT
  export PATH="$fixture/bin:$PATH"
  export FAKE_CARGO_LOG="$fixture/cargo-args" FAKE_PYTHON_LOG="$fixture/python-args"
  if [[ -n "$target_dir" ]]; then export CARGO_TARGET_DIR="$target_dir"; fi
  if [[ -n "$lib_override" ]]; then
    export GC_EFFECTS_LIB_DIR="$lib_override" GC_EFFECTS_SKIP_BUILD=1
    mkdir -p "$lib_override"
    : > "$lib_override/libperry_runtime.a"
    : > "$lib_override/libperry_stdlib.a"
  fi
  rm -f "$FAKE_CARGO_LOG" "$FAKE_PYTHON_LOG"
  # Invoke outside the fixture repo to exercise ROOT-relative Cargo paths.
  cd "$fixture"
  bash "$repo/scripts/gc_call_effects/regen.sh" "$target" --check
  local libs=(libperry_runtime.a libperry_stdlib.a)
  if [[ "$target" == windows-x86_64 ]]; then libs=(perry_runtime.lib perry_stdlib.lib); fi
  local expected actual
  expected="$(printf '%s\n' "$expected_dir/${libs[0]}" "$expected_dir/${libs[1]}")"
  actual="$(tail -n 2 "$FAKE_PYTHON_LOG")"
  [[ "$actual" == "$expected" ]] || {
    echo "$target: expected archive paths $expected; got $actual" >&2; exit 1;
  }
  if [[ -n "$lib_override" ]]; then
    [[ ! -e "$FAKE_CARGO_LOG" ]] || { echo "skip-build ran Cargo" >&2; exit 1; }
  else
    [[ -s "$FAKE_CARGO_LOG" ]] || { echo "fake Cargo was not exercised" >&2; exit 1; }
  fi
)

run_case macos-aarch64 "$fixture/absolute target with spaces" "$fixture/absolute target with spaces/release"
run_case linux-x86_64 "relative target with spaces" "relative target with spaces/release"
run_case linux-x86_64 "" "target/release"
run_case windows-x86_64 "" "target/x86_64-pc-windows-msvc/release"
run_case macos-aarch64 "$fixture/unused target" "$fixture/archive override" "$fixture/archive override"
echo "gc_call_effects regen path self-test passed: absolute, relative, defaults, explicit override"
