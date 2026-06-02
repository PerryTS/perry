// #2384: a value-level `Array.prototype.entries()`/`.keys()`/`.values()` must
// return a real `.next()`-bearing iterator OBJECT, not an eagerly materialized
// array. Before the fix, codegen's `Expr::ArrayEntries`/`ArrayKeys`/
// `ArrayValues` fast path lowered to `js_array_{entries,keys,values}` (a
// materialized array), so manual `e.next()` fell through the array-method
// catch-all that returns the receiver — `.next().value` was `undefined`.
// `for-of` and spread happened to work because they walk the materialized
// array directly.
//
// Fix: route the fast path to iterator-OBJECT constructors and route the
// `.next()`/`.return()`/`.throw()` array-method dispatch to the runtime's
// generic dispatcher (which reaches `dispatch_array_iterator_method`). Spread /
// for-of / Array.from keep working because `js_array_clone` /
// `js_for_of_to_array` already detect the iterator class id and drive `.next()`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) manual .next() on entries() yields { value: [i, x], done }.
const e = [10, 20, 30].entries();
console.log(JSON.stringify(e.next())); // {"value":[0,10],"done":false}
console.log(JSON.stringify(e.next())); // {"value":[1,20],"done":false}
console.log(JSON.stringify(e.next())); // {"value":[2,30],"done":false}
console.log(JSON.stringify(e.next())); // {"done":true}
console.log(e.next().value === undefined); // true (past end)

// (2) .keys() / .values() iterators advance independently.
const k = [10, 20, 30].keys();
console.log(JSON.stringify(k.next())); // {"value":0,"done":false}
console.log(k.next().value); // 1

const v = ["a", "b"].values();
console.log(v.next().value); // a
console.log(v.next().value); // b

// (3) the iterator is itself iterable, and spread / for-of / Array.from still
//     consume it correctly (regression guard for the materialized paths).
console.log(JSON.stringify([...[10, 20].entries()])); // [[0,10],[1,20]]
console.log(JSON.stringify([...[10, 20].keys()])); // [0,1]
console.log(JSON.stringify([...["a", "b"].values()])); // ["a","b"]

const out: string[] = [];
for (const [i, x] of ["a", "b"].entries()) {
  out.push(`${i}:${x}`);
}
console.log(out.join(",")); // 0:a,1:b

console.log(JSON.stringify(Array.from([10, 20].entries()))); // [[0,10],[1,20]]

// (4) early break out of a for-of over an iterator (exercises .return()).
for (const [i, x] of [9, 8, 7].entries()) {
  if (i === 1) break;
  console.log("brk", i, x); // brk 0 9
}

// (5) empty array iterator is immediately done.
console.log(JSON.stringify([].values().next())); // {"done":true}

// (6) chained: destructure the pair straight off .next().value.
const e2 = [100, 200].entries();
const [, first] = e2.next().value;
console.log(first); // 100
