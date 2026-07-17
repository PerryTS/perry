#!/usr/bin/env bash
# Thread-primitive regression tests (issue #146).
#
# Two halves:
#   1. Runtime: the docs/examples harness already compiles and stdout-diffs
#      docs/examples/runtime/thread_primitives.ts — not re-done here.
#   2. Compile-time safety: this script compiles small programs that the
#      compiler must reject (mutable outer captures passed to parallelMap /
#      parallelFilter / spawn). For each case, it asserts that compilation
#      exits non-zero AND that the stderr contains a specific error phrase.
#      If a future codegen change drops the check, the table is silent and
#      this script catches it.
#
# Usage:
#   scripts/run_thread_tests.sh                        # use ./target/release/perry
#   PERRY_BIN=/path/to/perry scripts/run_thread_tests.sh

set -uo pipefail

PERRY_BIN="${PERRY_BIN:-$(pwd)/target/release/perry}"
if [[ ! -x "$PERRY_BIN" ]]; then
    echo "perry binary not found at $PERRY_BIN; set PERRY_BIN or run 'cargo build --release -p perry'"
    exit 2
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

pass=0
fail=0
expect_compile_error() {
    local name="$1"
    local src_path="$2"
    local expected_substring="$3"

    local stderr_path="$TMP_DIR/$name.stderr"
    if "$PERRY_BIN" "$src_path" -o "$TMP_DIR/$name.out" >/dev/null 2>"$stderr_path"; then
        echo "FAIL $name: compile succeeded but should have errored"
        fail=$((fail+1))
        return
    fi
    if ! grep -qF "$expected_substring" "$stderr_path"; then
        echo "FAIL $name: error message missing expected substring"
        echo "  expected: $expected_substring"
        echo "  actual stderr:"
        sed 's/^/    /' "$stderr_path"
        fail=$((fail+1))
        return
    fi
    echo "PASS $name"
    pass=$((pass+1))
}

# Case 1: parallelMap with mutable outer capture — must fail.
cat >"$TMP_DIR/parallelMap_mutates.ts" <<'EOF'
import { parallelMap } from "perry/thread";
let counter = 0;
const data = [1, 2, 3, 4];
const out = parallelMap(data, (item: number) => {
    counter = counter + 1;
    return item * 2;
});
console.log(out.length);
console.log(counter);
EOF
expect_compile_error parallelMap_mutates \
    "$TMP_DIR/parallelMap_mutates.ts" \
    "perry/thread: closure passed to \`parallelMap\` writes to outer variable"

# Case 2: parallelFilter with mutable outer capture — must fail.
cat >"$TMP_DIR/parallelFilter_mutates.ts" <<'EOF'
import { parallelFilter } from "perry/thread";
let rejected = 0;
const data = [1, 2, 3, 4, 5, 6, 7, 8];
const out = parallelFilter(data, (x: number) => {
    if (x % 2 !== 0) {
        rejected = rejected + 1;
    }
    return x % 2 === 0;
});
console.log(out.length);
console.log(rejected);
EOF
expect_compile_error parallelFilter_mutates \
    "$TMP_DIR/parallelFilter_mutates.ts" \
    "perry/thread: closure passed to \`parallelFilter\` writes to outer variable"

# Case 3: spawn with mutable outer capture — must fail.
cat >"$TMP_DIR/spawn_mutates.ts" <<'EOF'
import { spawn } from "perry/thread";
let total = 0;
async function main(): Promise<void> {
    await spawn(() => {
        total = total + 42;
        return total;
    });
    console.log(total);
}
main();
EOF
expect_compile_error spawn_mutates \
    "$TMP_DIR/spawn_mutates.ts" \
    "perry/thread: closure passed to \`spawn\` writes to outer variable"

# Case 4: const capture is FINE — must compile. Value-only captures are safe
# because a deep-copied snapshot is exactly what the worker needs; there's
# nothing the closure could write back.
cat >"$TMP_DIR/const_capture_ok.ts" <<'EOF'
import { parallelMap } from "perry/thread";
const rate = 1.08;
const data = [100, 200, 300];
const out = parallelMap(data, (x: number) => x * rate);
console.log(out.length);
EOF
if "$PERRY_BIN" "$TMP_DIR/const_capture_ok.ts" -o "$TMP_DIR/const_capture_ok.out" >/dev/null 2>"$TMP_DIR/const_capture_ok.stderr"; then
    echo "PASS const_capture_ok"
    pass=$((pass+1))
else
    echo "FAIL const_capture_ok: const captures should compile"
    sed 's/^/    /' "$TMP_DIR/const_capture_ok.stderr"
    fail=$((fail+1))
fi

# Case 5 (#6518, family of #6486): an array push-grown past its inline
# capacity (16) inside a helper leaves the caller's slot holding a stale
# pre-grow pointer (js_array_grow forwarding stub, #233). parallelMap /
# parallelFilter read raw `(*arr).length` on the input, and serialize_array
# raw-read it on any array crossing the thread boundary (as an element on the
# way in, or as a worker's return value on the way out) — in every case
# reading the forwarding pointer's bytes as the element count. Must compile,
# run, and produce exact output.
cat >"$TMP_DIR/grown_array_crossing.ts" <<'EOF'
import { parallelFilter, parallelMap, spawn } from "perry/thread";

function fill(out: number[], a: number[]): void {
  const vs = [a, a, a, a, a, a];
  for (const v of vs) out.push(v[0], v[1], v[2]);
}

const verts: number[] = [];
fill(verts, [1, 2, 3]);

// parallel_map_impl / parallel_filter_impl: raw length read on the input.
const doubled = parallelMap(verts, (x: number) => x * 2);
console.log(doubled.length, doubled[0], doubled[17]);

const twos = parallelFilter(verts, (x: number) => x === 2);
console.log(twos.length, twos[0]);

// serialize_array, main-thread side: grown rows as crossing elements.
const matrix = [verts, verts, verts, verts];
const lens = parallelMap(matrix, (r: number[]) => r.length);
console.log(lens.length, lens[0], lens[3]);

// Holes cross as undefined: element serialization reads through the
// canonical accessor, which normalizes the hole sentinel.
const holey: number[] = [];
holey[20] = 5;
const marks = parallelMap(holey, (x: number | undefined) => (x === undefined ? 1 : 2));
console.log(marks.length, marks[0], marks[20]);

// serialize_array, worker side: the worker's own slot goes stale the same
// way (fill grows `out` past 16), then the return value crosses back.
async function main(): Promise<void> {
  const back: number[] = await spawn(() => {
    const out: number[] = [];
    fill(out, [4, 5, 6]);
    return out;
  });
  console.log(back.length, back[0], back[17]);
}
await main();
EOF
expected_grown_output=$'18 2 6\n6 2\n4 18 18\n21 1 2\n18 4 6'
if ! "$PERRY_BIN" "$TMP_DIR/grown_array_crossing.ts" -o "$TMP_DIR/grown_array_crossing.out" \
        >/dev/null 2>"$TMP_DIR/grown_array_crossing.stderr"; then
    echo "FAIL grown_array_crossing: compile error"
    sed 's/^/    /' "$TMP_DIR/grown_array_crossing.stderr"
    fail=$((fail+1))
elif actual_grown_output="$("$TMP_DIR/grown_array_crossing.out" 2>&1)" \
        && [[ "$actual_grown_output" == "$expected_grown_output" ]]; then
    echo "PASS grown_array_crossing"
    pass=$((pass+1))
else
    echo "FAIL grown_array_crossing: wrong runtime output"
    echo "  expected:"
    sed 's/^/    /' <<<"$expected_grown_output"
    echo "  actual:"
    sed 's/^/    /' <<<"$actual_grown_output"
    fail=$((fail+1))
fi

echo
echo "thread-tests: $pass passed, $fail failed"

# release_sweep.sh hook — see comment in run_parity_tests.sh.
if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_thread_tests.sh", "passed": $pass, "failed": $fail, "skipped": 0}
EOF
fi

[[ $fail -eq 0 ]]
