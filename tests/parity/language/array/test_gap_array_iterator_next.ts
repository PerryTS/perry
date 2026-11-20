// #321 (effect `ManagedRuntime` / `Exit.all`): a value-level
// `arr[Symbol.iterator]()` / `arr.values()` / `arr.keys()` / `arr.entries()`
// call must return a real `.next()`-bearing iterator (matching Node), and
// `Array.from` must prefer the iterator protocol over an array-like `.length`.
//
// Before the fix the runtime dispatch tower had no `values`/`keys`/`entries`/
// `@@iterator` arm for arrays, so a dynamic `arr[Symbol.iterator]()` returned
// `undefined`; and `Array.from(obj)` over an iterable carrying a `.length`
// (e.g. effect's `Chunk`) read `.length`/`obj[i]` and produced N `undefined`
// elements. That surfaced as `Cannot read properties of undefined (reading
// '_tag')` inside effect's `exitZipWith`.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

const arr = [10, 20, 30];

// (1) arr[Symbol.iterator]() returns an iterator with a working .next().
const it = arr[Symbol.iterator]();
const a = it.next();
const b = it.next();
console.log("next0:", a.done, a.value);
console.log("next1:", b.done, b.value);

// (2) arr.values()/keys()/entries() drive the same iterator protocol when
//     consumed via Array.from / spread.
console.log("values:", Array.from(arr.values()).join(","));
console.log("keys:", Array.from(arr.keys()).join(","));
console.log("entries:", JSON.stringify(Array.from(arr.entries())));

// (3) Array.from over a custom iterable that DELEGATES to a backing array's
//     iterator (the effect Chunk shape).
const backing = [{ tag: "A" }, { tag: "B" }];
const iterable: Iterable<{ tag: string }> = {
  [Symbol.iterator]() {
    return backing[Symbol.iterator]();
  },
};
const fromIter = Array.from(iterable);
console.log("fromIter len:", fromIter.length);
console.log("fromIter tags:", fromIter.map((x) => x.tag).join(","));

// (4) spread of the same iterable.
const spread = [...iterable];
console.log("spread tags:", spread.map((x) => x.tag).join(","));

// (5) Array.from over an object that is BOTH array-like (.length) AND iterable
//     must take the iterator path (spec §23.1.2.1), not the .length path.
const dual: any = {
  length: 99, // a lie — the iterator yields the real elements
  [Symbol.iterator]() {
    return [7, 8, 9][Symbol.iterator]();
  },
};
console.log("dual:", Array.from(dual).join(","));

// (6) reduce over an array iterator's materialized values still works.
const total = Array.from(arr.values()).reduce((acc, n) => acc + n, 0);
console.log("reduce total:", total);
