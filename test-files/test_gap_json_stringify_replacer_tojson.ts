// JSON.stringify replacer path: must run toJSON BEFORE the replacer
// (SerializeJSONProperty step 2), honor the indent/space arg, and not crash.
// Regression for the #321-frontier replacer divergence.

// toJSON (own-field) resolved before an identity replacer.
const o: any = { toJSON() { return { id: "X", items: [1, 2] }; } };
console.log(JSON.stringify(o, (k, v) => v)); // {"id":"X","items":[1,2]}

// Same, with indent — replacer path must pretty-print.
console.log(JSON.stringify(o, (k, v) => v, 2));
// {
//   "id": "X",
//   "items": [
//     1,
//     2
//   ]
// }

// Indent threads through a nested array under a replacer.
console.log(JSON.stringify({ a: [1, 2] }, (k, v) => v, 2));
// {
//   "a": [
//     1,
//     2
//   ]
// }

// Nested toJSON inside a replacer + indent.
const nested: any = { wrap: { toJSON() { return [9, 8]; } } };
console.log(JSON.stringify(nested, (k, v) => v, 2));
// {
//   "wrap": [
//     9,
//     8
//   ]
// }

// Replacer drops keys (returns undefined) — still works post-fix.
console.log(
  JSON.stringify({ a: 1, b: 2, c: 3 }, (k, v) =>
    typeof v === "number" && v > 1 ? undefined : v,
  ),
); // {"a":1,"c":3}

// Replacer transforms values.
console.log(
  JSON.stringify({ a: 1, b: 2 }, (k, v) => (typeof v === "number" ? v * 10 : v)),
); // {"a":10,"b":20}

// Array of objects + replacer + indent.
console.log(
  JSON.stringify({ list: [{ x: 1 }, { x: 2 }], n: 3 }, (k, v) => v, 2),
);
// {
//   "list": [
//     {
//       "x": 1
//     },
//     {
//       "x": 2
//     }
//   ],
//   "n": 3
// }

// Top-level array with replacer + indent.
console.log(JSON.stringify([1, { a: 2 }, [3, 4]], (k, v) => v, 2));
// [
//   1,
//   {
//     "a": 2
//   },
//   [
//     3,
//     4
//   ]
// ]

// Mixed scalar values through a replacer.
console.log(JSON.stringify({ s: "hi", b: true, n: null, x: 5 }, (k, v) => v));
// {"s":"hi","b":true,"n":null,"x":5}

// The effect-style stringifyCircular shape: function replacer with a dedup
// cache + whitespace, over objects that expose toJSON. Must not crash.
const inspectable: any = { _tag: "Box", toJSON() { return { _tag: "Box", value: 42 }; } };
let cache: any[] = [];
console.log(
  JSON.stringify(
    inspectable,
    (_k, v) =>
      typeof v === "object" && v !== null
        ? cache.includes(v)
          ? undefined
          : (cache.push(v), v)
        : v,
    2,
  ),
);
// {
//   "_tag": "Box",
//   "value": 42
// }
