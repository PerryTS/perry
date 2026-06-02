// JSON.stringify of Map/Set values (#321).
//
// Per spec, Map and Set have no own enumerable string-keyed properties and no
// toJSON, so JSON.stringify serializes them as "{}" (empty object) — at the
// root, as an object field, as an array element, and through the indent,
// function-replacer, and array (key-whitelist) replacer paths. Previously
// Perry mis-read the Map/Set header as a plain object and dereferenced its
// internals as a keys_array pointer, segfaulting the process.

// Root value
console.log(JSON.stringify(new Map([["a", 1], ["b", 2]])));
console.log(JSON.stringify(new Set([1, 2, 3])));

// As an object field (mixed with serializable siblings)
console.log(JSON.stringify({ m: new Map([["x", 1]]), n: 5 }));
console.log(JSON.stringify({ s: new Set([1, 2]), n: 5 }));

// As an array element
console.log(JSON.stringify([new Map(), 7]));
console.log(JSON.stringify([new Set([1]), 8]));

// Indent / pretty-print path
console.log(JSON.stringify({ m: new Map([["x", 1]]) }, null, 2));
console.log(JSON.stringify(new Map([["a", 1]]), null, 2));

// Function-replacer path (identity replacer)
console.log(JSON.stringify({ m: new Map([["x", 1]]), keep: 9 }, (k, v) => v));
console.log(JSON.stringify(new Map([["x", 1]]), (k, v) => v));

// Array (key-whitelist) replacer path
console.log(JSON.stringify({ m: new Map([["x", 1]]), keep: 9 }, ["m", "keep"]));
console.log(JSON.stringify(new Map([["x", 1]]), ["m"]));

// Deeply nested Map/Set
console.log(JSON.stringify({ outer: { inner: new Map([["k", new Set([1, 2])]]) } }));

// Expected output:
// {}
// {}
// {"m":{},"n":5}
// {"s":{},"n":5}
// [{},7]
// [{},8]
// {
//   "m": {}
// }
// {}
// {"m":{},"keep":9}
// {}
// {"m":{},"keep":9}
// {}
// {"outer":{"inner":{}}}
