// An array that never escapes is scalar-replaced: its elements live in stack
// slots and the local never holds a heap array. `lower_numeric_index_get_for_
// number_context` — the path taken when an element read feeds arithmetic — did
// not check for that, so it lowered `a[i]` through the guarded element path with
// the empty local as the receiver. The runtime guard saw a null array and
// declined, and the boxed fallback coerced the resulting `undefined` to NaN.
//
// `a[0]` on its own was correct (`lower` serves it from the scalar slot), so
// only the arithmetic form was wrong:
//
//     const a = [1, 2, 3];
//     a[0];        // 1   ✓
//     a[0] + 1;    // NaN ✗   (expected 2)

const m = [1, 2, 3];
if (m[0] + 1 !== 2) throw new Error(`module-level a[0] + 1 = ${m[0] + 1}, expected 2`);
if (m[1] * 2 !== 4) throw new Error(`module-level a[1] * 2 = ${m[1] * 2}, expected 4`);
if (m[0] !== 1) throw new Error(`module-level a[0] = ${m[0]}, expected 1`);

function local() {
  const a = [10, 20, 30];
  return a[2] - a[0];
}
if (local() !== 20) throw new Error(`function-local a[2] - a[0] = ${local()}, expected 20`);

let mutable = [1.5, 2.5];
if (mutable[0] * 2 !== 3) throw new Error(`float array a[0] * 2 = ${mutable[0] * 2}, expected 3`);

// An array that DOES escape keeps its heap allocation — this path already worked
// and must keep working.
const escaping = [1, 2, 3];
function readsEscaping() {
  return escaping[0] + 1;
}
if (readsEscaping() !== 2) throw new Error(`escaping a[0] + 1 = ${readsEscaping()}, expected 2`);

// Elements feeding arithmetic inside a loop.
const nums = [1, 2, 3, 4];
let total = 0;
for (let i = 0; i < nums.length; i++) {
  total += nums[i] * 2;
}
if (total !== 20) throw new Error(`sum of nums[i] * 2 = ${total}, expected 20`);

console.log("scalar-array number context ok");
