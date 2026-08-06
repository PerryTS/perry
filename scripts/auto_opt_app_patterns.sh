#!/usr/bin/env bash
# Compile the `benchmarks/app-patterns` kernels through the AUTO-OPTIMIZE path
# and diff their output against the pinned Node oracle.
#
# WHY THIS EXISTS (#7475)
#
# The auto-optimize path — the default, i.e. exactly how `perry file.ts -o out`
# behaves and how the benchmark harness invokes it — rebuilds perry-runtime and
# perry-stdlib with a per-app Cargo feature set into `target/perry-auto-<hash>/`
# and links THOSE archives over whatever `PERRY_RUNTIME_DIR` points at. That is
# a different binary from the one every other gate in this repo tests:
# `gc-ratchet`, the gap suite's hand-rolled probes and most `crates/perry/tests`
# cases all set `PERRY_NO_AUTO_OPTIMIZE=1` for a deterministic link.
#
# #7475 is what that blind spot costs. `object_deep_clone` threw
# `TypeError: next is not a function` under auto-optimize while printing the
# correct checksum under `PERRY_NO_AUTO_OPTIMIZE=1`. The defect was in neither
# the kernel nor the feature set: `js_iterator_to_array` held the iterator, the
# accumulator array and the result object in bare Rust locals across a `.next()`
# call that allocates, so a copying minor moved them and left the pre-move
# addresses behind. Both builds had it — `PERRY_GC_PROTECT_FROMSPACE=1` faults
# on both — but only the feature-stripped one allocated in the order that made
# the stale read observable. A latent stale-root read is invisible until some
# unrelated change perturbs allocation timing, and the auto-optimize link
# perturbs it on every app.
#
# THIS SCRIPT IS DESIGNED TO BE ABLE TO FAIL. In particular it ASSERTS ITS
# SUBJECT WAS LIVE rather than assuming it: a run in which the auto-optimizer
# silently fell back to the prebuilt archives would exercise nothing this gate
# exists for while passing every output comparison. So for each kernel it
# requires the linker command line (from `perry -v`) to name a
# `perry-auto-*/…/libperry_runtime.a` that exists on disk. No archive, no pass.
#
# Usage:
#   scripts/auto_opt_app_patterns.sh              # every kernel except the skips
#   scripts/auto_opt_app_patterns.sh promise_all_chains   # named kernels only
#
# Environment:
#   PERRY_BIN   perry binary to test (default: target/release/perry)
#   NODE_BIN    node oracle (default: node)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PERRY_BIN="${PERRY_BIN:-$ROOT/target/release/perry}"
NODE_BIN="${NODE_BIN:-node}"
KERNEL_DIR="$ROOT/benchmarks/app-patterns/kernels"

# `PERRY_NO_AUTO_OPTIMIZE` would defeat the entire point of this gate, and it is
# set by several sibling scripts and CI jobs. Refuse rather than inherit it.
if [[ -n "${PERRY_NO_AUTO_OPTIMIZE:-}" ]]; then
  echo "error: PERRY_NO_AUTO_OPTIMIZE is set; this gate tests the auto-optimize path." >&2
  exit 2
fi

# Kernels excluded from the gate, one `name:reason` per line. An entry that
# names a kernel which no longer exists FAILS below — the same rule
# `scripts/gc_root_dominance_allowlist.json` uses, so a fixed kernel cannot keep
# its exemption by inertia.
SKIPS=(
  # #7475: fails under BOTH link modes, differently ("Uncaught (in promise) 0"
  # with PERRY_NO_AUTO_OPTIMIZE=1, a rooting-shaped TypeError without). The
  # promise-rejection half is a separate defect from the iterator-drain rooting
  # bug this gate was created for; it is tracked on #7475 and this line comes
  # out with it.
  "promise_all_chains:pre-existing promise-rejection failure, #7475"
)

if [[ ! -x "$PERRY_BIN" ]]; then
  echo "error: perry binary not found at $PERRY_BIN (set PERRY_BIN)" >&2
  exit 2
fi
if ! command -v "$NODE_BIN" >/dev/null 2>&1; then
  echo "error: node oracle '$NODE_BIN' not found (set NODE_BIN)" >&2
  exit 2
fi

all_kernels=()
for f in "$KERNEL_DIR"/*.ts; do
  all_kernels+=("$(basename "$f" .ts)")
done
if [[ ${#all_kernels[@]} -eq 0 ]]; then
  echo "error: no kernels found in $KERNEL_DIR" >&2
  exit 2
fi

skip_names=()
for entry in "${SKIPS[@]}"; do
  skip_names+=("${entry%%:*}")
done

# A skip entry that matches nothing is a failure, not a no-op.
rotted=0
for name in "${skip_names[@]}"; do
  found=0
  for k in "${all_kernels[@]}"; do
    [[ "$k" == "$name" ]] && found=1
  done
  if [[ $found -eq 0 ]]; then
    echo "error: skip entry '$name' matches no kernel in $KERNEL_DIR — delete its line." >&2
    rotted=1
  fi
done
[[ $rotted -eq 0 ]] || exit 1

requested=("$@")
selected=()
if [[ ${#requested[@]} -gt 0 ]]; then
  selected=("${requested[@]}")
else
  for k in "${all_kernels[@]}"; do
    skip=0
    for name in "${skip_names[@]}"; do
      [[ "$k" == "$name" ]] && skip=1
    done
    [[ $skip -eq 1 ]] || selected+=("$k")
  done
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
echo "auto-optimize app-pattern gate: ${#selected[@]} kernel(s), perry=$PERRY_BIN"
echo

for name in "${selected[@]}"; do
  src="$KERNEL_DIR/$name.ts"
  if [[ ! -f "$src" ]]; then
    echo "FAIL  $name — no such kernel at $src"
    failures=$((failures + 1))
    continue
  fi

  out="$WORK/$name"
  log="$WORK/$name.compile.log"
  # `-v` makes the driver echo the linker command line, which is the only
  # first-hand evidence of WHICH runtime archive the binary actually links.
  if ! "$PERRY_BIN" -v "$src" -o "$out" >"$log" 2>&1; then
    echo "FAIL  $name — compile failed:"
    sed 's/^/        /' "$log" | tail -30
    failures=$((failures + 1))
    continue
  fi

  # LIVENESS. Pull the auto-optimize archive out of the linker invocation, not
  # out of a status message: a message can be printed by a path that then falls
  # back, the link line cannot.
  archive="$(grep -o '[^ ]*perry-auto-[^ ]*libperry_runtime\.a' "$log" | head -1 || true)"
  if [[ -z "$archive" ]]; then
    echo "FAIL  $name — the link line names no perry-auto-*/libperry_runtime.a."
    echo "        The auto-optimizer fell back to a prebuilt archive, so this run"
    echo "        exercised nothing this gate exists for. Compile log tail:"
    grep -E 'auto-optimize|\[link\] invoking' "$log" | sed 's/^/        /' | tail -10
    failures=$((failures + 1))
    continue
  fi
  if [[ ! -s "$archive" ]]; then
    echo "FAIL  $name — link named $archive but it is missing or empty."
    failures=$((failures + 1))
    continue
  fi

  actual="$("$out" 2>&1)" && perry_status=0 || perry_status=$?
  expected="$("$NODE_BIN" --experimental-strip-types "$src" 2>&1)" && node_status=0 || node_status=$?

  if [[ $node_status -ne 0 ]]; then
    echo "FAIL  $name — the NODE oracle exited $node_status; the comparison would be"
    echo "        vacuous. Check the pinned node in .node-version."
    echo "$expected" | sed 's/^/        /' | tail -10
    failures=$((failures + 1))
    continue
  fi
  if [[ $perry_status -ne 0 ]]; then
    echo "FAIL  $name — the compiled binary exited $perry_status"
    echo "$actual" | sed 's/^/        /' | tail -10
    failures=$((failures + 1))
    continue
  fi
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL  $name — output differs from node"
    echo "        perry: $actual"
    echo "        node : $expected"
    failures=$((failures + 1))
    continue
  fi

  echo "PASS  $name  ($(basename "$(dirname "$(dirname "$archive")")"))"
done

echo
for entry in "${SKIPS[@]}"; do
  echo "SKIP  ${entry%%:*} — ${entry#*:}"
done

if [[ $failures -gt 0 ]]; then
  echo
  echo "$failures kernel(s) failed under the auto-optimize link."
  exit 1
fi
echo
echo "OK: every selected kernel linked a freshly built perry-auto runtime archive"
echo "    and matched the node oracle byte for byte."
