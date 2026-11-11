// VR-4b S0 arm `vrarrjoin` (DESIGN-p4l4 §10.3, finding 1): one body over a
// record array and over a Number array. Correct: R6Join rejects the record
// (a holder of a record array holds nothing else), which stays a heap
// object in an ordinary Array. Sabotaged (the join ignored): `len`'s
// parameter is typed a record array, its `length` is the record-array
// intrinsic, and check mode reports the Number array it receives with 86.
// The helper calls itself so that the inliner leaves the calls in place.
function len(a: unknown[], depth: number): number {
  if (depth > 0) return len(a, depth - 1);
  return a.length;
}
function mk(n: number): { a: number; b: number } {
  return { a: n, b: n + 1 };
}
const undos: { a: number; b: number }[] = [];
for (let i = 0; i < 3; i++) undos.push(mk(i));
const last = undos.pop();
console.log(len(undos, 1), len([1, 2, 3, 4], 1), last === undefined ? -1 : last.a + last.b);
