// Plain Array births and the fused fill (DESIGN-p4l1 §2.3, §2.6, PR-B):
// every shape node and Perry agree on, read inside and past the length.

function sum(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = i;
    const v = a[k];
    s = s + (v === undefined ? 1000 : v);
  }
  return s;
}

// The fused fill: every index below the length holds the value.
const fused = new Array(8).fill(3);
console.log("fused", sum(fused, 8), sum(fused, 10), fused.length);

// Two arguments: not fused, index 0 stays a hole.
const partial = new Array(4).fill(0, 1);
console.log("partial", partial[0] ?? 9, sum(partial, 4), 0 in partial, 1 in partial);

// A non-literal length.
const n = 3 + (process.argv.length > 100 ? 1 : 0);
const sized = new Array(n).fill(2);
console.log("sized", sum(sized, n), sized.length);

// Over the dense limit: length 2,000,000 over a store fill densifies.
const big = new Array(2_000_000).fill(7);
console.log("big", big[0], big[1_999_999], big[2_000_000] ?? -1, big.length);

// Without `new`, and a fill on a binding.
const bare = Array(4).fill(1);
const later: number[] = new Array(4);
const back = later.fill(5);
console.log("bare", sum(bare, 4), sum(later, 4), back === later);

// Holes: a holey literal, `Array(4)`, `Array.from({ length: 4 })`.
const holey = [1, , 3];
const empty = Array(4);
const from = Array.from({ length: 4 });
console.log("holey", holey[1] ?? 9, 1 in holey, holey.length, sum(holey as number[], 3));
console.log("empty", empty[2] ?? 9, 2 in empty, empty.length);
console.log("from", from[2] ?? 9, 2 in from, from.length);

// Stored `undefined` is an element, not a hole.
const stored: (number | undefined)[] = [1, 2, 3];
stored[1] = undefined;
const x = stored[1];
console.log("stored", stored[1] ?? 5, typeof x, 1 in stored);

// A literal of many elements and a spread literal.
const lit = [5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21];
const spread = [...lit, 22];
console.log("lit", sum(lit, 17), sum(spread, 18), spread.length);

// `new Array(a, b)` and `new Array("x")` are element lists.
const two = new Array(4, 5);
const one = new Array("x");
console.log("args", two.length, two[0], two[1], one.length, one[0]);
