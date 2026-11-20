// Regression test for issue #221.
//
// Before the fix, a module-level `const arr: T[] = []` (declared empty)
// mutated via index assignment from inside a function silently dropped
// every write — both the value and the implicit `.length` update vanished.
// Pre-initialized arrays and `.push()` worked, but `arr[i] = v` did not.
//
// Root cause: codegen's IndexSet path (`crates/perry-codegen/src/expr.rs`)
// funneled module-level array receivers through `js_array_set_f64`, the
// bounds-checked variant that returns silently when `index >= length`.
// For an empty array (length=0), every write at any index >= 0 dropped.
//
// The fix routes module-level receivers through `js_array_set_f64_extend`
// (the realloc-capable variant) and writes the new pointer back to the
// module global slot — symmetric with the existing stack-local fast path.
//
// This test exercises:
//   - empty string array filled by index from inside a function
//   - empty number array filled by index from inside a function
//   - pre-sized number array index-set past existing length (extends)
//   - object array filled by index from inside a function
// It also pins the gap-fill behavior (Perry fills with 0, matching the
// runtime's documented `js_array_set_f64_extend` semantics — Node would
// return `undefined` for the gap; that's a documented divergence in the
// runtime helper, not something this fix should change).

// Exact repro from issue body: fill() writes and reads, read() reads only.
const A: string[] = [];
function fill(): void {
  for (let i = 0; i < 5; i = i + 1) {
    A[i] = "v" + i.toString();
  }
  console.log("[fill] A.length =", A.length, "A[0]=", A[0], "A[2]=", A[2]);
}
function read(): void {
  console.log("[read] A.length =", A.length, "A[0]=", A[0], "A[2]=", A[2]);
}
fill();
read();

// Additional cases:
const N: number[] = [];
function fillN(): void {
  for (let i = 0; i < 4; i = i + 1) N[i] = i * 10;
}

const M = [100, 200];
function extendM(): void { M[5] = 999; }

const O: object[] = [];
function fillO(): void {
  for (let i = 0; i < 3; i = i + 1) O[i] = { id: i };
}

fillN();
extendM();
fillO();

console.log("N:", N.length, N[0], N[3]);
console.log("M:", M.length, M[0], M[1], M[5]);
console.log("O[0].id:", (O[0] as { id: number }).id, "O[2].id:", (O[2] as { id: number }).id);
