// Gap test for #2773 (Array.from validation + mapped semantics),
// #2805 (Array.prototype.concat variadic, non-mutating, isConcatSpreadable),
// and #2774 (Uint8Array.from / Uint8Array.of produce typed arrays).
// Compared byte-for-byte against `node --experimental-strip-types`.

// ---- #2773: Array.from ----
console.log(JSON.stringify(Array.from({ 0: "a", 2: "c", length: 3 })));
console.log(JSON.stringify(Array.from("ab")));
console.log(
  JSON.stringify(
    Array.from(
      [10, 20],
      function (value, index) {
        return [this.mult * value, index].join(":");
      },
      { mult: 2 },
    ),
  ),
);
console.log(JSON.stringify(Array.from([1, 2], (value, index) => value + index)));

try {
  Array.from(null as any);
} catch (e) {
  console.log("from null: " + (e as Error).name);
}
try {
  Array.from(undefined as any);
} catch (e) {
  console.log("from undefined: " + (e as Error).name);
}
try {
  Array.from([1], 1 as any);
} catch (e) {
  console.log("from bad mapFn: " + (e as Error).name);
}

// ---- #2805: Array.prototype.concat ----
console.log(JSON.stringify([1].concat([2, 3])));
console.log(JSON.stringify([1].concat([2], [3, 4], 5)));
console.log(JSON.stringify([1].concat()));
{
  const a = [1];
  const out = a.concat([2]);
  console.log(JSON.stringify([out, a, out === a]));
}
{
  // Array with Symbol.isConcatSpreadable === false -> single element.
  // (Set via defineProperty: bracket-assigning a Symbol key onto an Array
  // is a separate, unrelated Perry index-set bug.)
  const x: any = [2, 3];
  Object.defineProperty(x, Symbol.isConcatSpreadable, { value: false });
  console.log(JSON.stringify([1].concat(x)));
}
{
  const x: any = { 0: "a", 1: "b", length: 2, [Symbol.isConcatSpreadable]: true };
  console.log(JSON.stringify([1].concat(x)));
}

// ---- #2774: Uint8Array.from / .of ----
{
  const a = Uint8Array.from([1, 257, -1, 3.9]);
  console.log(a.length, JSON.stringify([...a]));
}
{
  const b = Uint8Array.of(1, 257, -1, 3.9);
  console.log(b.length, JSON.stringify([...b]));
}
console.log(JSON.stringify([...Uint8Array.from("ABC", (c) => c.charCodeAt(0))]));
console.log(
  JSON.stringify([
    ...Uint8Array.from(
      [1, 2],
      function (v, i) {
        return v * this.mult + i;
      },
      { mult: 10 },
    ),
  ]),
);
