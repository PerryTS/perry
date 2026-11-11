// Gap test: Array.prototype.splice reached via the dynamic dispatch tower.
// Run: node --experimental-strip-types test_gap_array_splice_dispatch.ts
//
// When the receiver is genuinely `any`-typed, `arr.splice(...)` lowers through
// the runtime method-dispatch tower (`js_native_call_method`) rather than the
// HIR array fast path. Before the fix, that tower had arms for slice/sort/
// reverse but NOT splice, so `(arr as any).splice(...)` threw
// "splice is not a function". (#321 effect Context/Layer surfaced this.)

// --- delete in the middle ---
const a1: any = [1, 2, 3, 4, 5];
const removed1 = a1.splice(1, 2);
console.log("removed:", removed1); // [2, 3]
console.log("after:", a1); // [1, 4, 5]

// --- insert without deleting ---
const a2: any = [1, 2, 5];
const removed2 = a2.splice(2, 0, 3, 4);
console.log("removed (insert):", removed2); // []
console.log("after (insert):", a2); // [1, 2, 3, 4, 5]

// --- replace (delete + insert) ---
const a3: any = [1, 2, 3];
const removed3 = a3.splice(1, 1, 99);
console.log("removed (replace):", removed3); // [2]
console.log("after (replace):", a3); // [1, 99, 3]

// --- negative start ---
const a5: any = [1, 2, 3, 4];
const removed5 = a5.splice(-2, 1);
console.log("removed (neg start):", removed5); // [3]
console.log("after (neg start):", a5); // [1, 2, 4]

// --- splice on a fresh array returned from an any-typed call ---
function makeArr(): any {
  return ["a", "b", "c", "d"];
}
const a6: any = makeArr();
const removed6 = a6.splice(1, 2, "X");
console.log("removed (call recv):", removed6); // ['b', 'c']
console.log("after (call recv):", a6); // ['a', 'X', 'd']
