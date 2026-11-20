// #6518 (forwarding-chain family of #6486): an array grown by `push` past its
// inline capacity (16) inside a helper moves via `js_array_grow`, leaving a
// GC_FLAG_FORWARDED stub at the old address (#233) while the caller's
// variable keeps the stale pre-grow pointer. Spreading that array into a call
// is only safe because SOMETHING on the path follows the forwarding chain —
// today `js_array_like_to_array`'s real-Array arm (clean_arr_ptr), plus the
// #6518 re-clean inside `js_closure_call_apply_with_spread` itself. This test
// pins the end-to-end behavior so neither resolution point can silently
// regress: `f(...arr)` reading the stub's bytes as the spread length crashes
// or garbles every line below.

// The #6486 trigger shape: fn + for-of + 3-arg push × 6 iterations grows the
// caller's array past 16 while the caller's slot keeps the pre-grow pointer.
function fill(out: number[], a: number[]): void {
  const vs = [a, a, a, a, a, a];
  for (const v of vs) out.push(v[0], v[1], v[2]);
}
const verts: number[] = [];
fill(verts, [1, 2, 3]);
console.log(verts.length);

// Closure spread call — the CallSpread arm that lowers to
// js_closure_call_apply_with_spread.
const collect = (...xs: number[]): number => xs.length;
console.log(collect(...verts));

// Regular args mixed with spread.
console.log(collect(0, 0, ...verts));

// Spread into declared params + rest: the values (not just the count) must
// come from the grown array's real elements.
const sum3 = (a: number, b: number, c: number, ...rest: number[]): number =>
  a + b + c + rest.length;
console.log(sum3(...verts));

// Builtin spread callees over the same grown array.
console.log(Math.max(...verts), Math.min(...verts));

// Below-capacity control: must keep working (never corrupted before either).
function fillSmall(out: number[], a: number[]): void {
  const vs = [a, a, a];
  for (const v of vs) out.push(v[0], v[1], v[2]);
}
const small: number[] = [];
fillSmall(small, [4, 5, 6]);
console.log(collect(...small), sum3(...small));

// Holes must spread as `undefined`: the spread slots are read through the
// canonical element accessor, which normalizes the hole sentinel — a raw
// slot copy would leak the sentinel bits into the argument buffer.
const holey: number[] = [];
holey[20] = 9;
const pick = (...xs: (number | undefined)[]): string => `${xs.length} ${xs[0]} ${xs[20]}`;
console.log(pick(...holey));
console.log(pick(7, ...holey));
