// Issue #2438 — own-property enumeration order. ECMA-262
// OrdinaryOwnPropertyKeys lists array-index keys (canonical numeric strings
// "0".."2^32-2") first in ascending numeric order, THEN the remaining string
// keys in insertion order. Perry previously kept a single insertion-ordered
// keys array, so integer-index keys came out in insertion order. This affects
// Object.keys / Object.values / Object.entries / JSON.stringify / for-in.
// All lines compare byte-for-byte against `node --experimental-strip-types`.

const o: any = {};
o[2] = "a";
o[1] = "b";
o[10] = "c";
o.x = "d";
o[3] = "e";

// Integer keys ascend, then string keys in insertion order.
console.log(Object.keys(o)); // [ '1', '2', '3', '10', 'x' ]
console.log(JSON.stringify(o)); // {"1":"b","2":"a","3":"e","10":"c","x":"d"}
console.log(Object.values(o)); // [ 'b', 'a', 'e', 'c', 'd' ]
console.log(JSON.stringify(Object.entries(o)));

// for-in walks the same spec order.
const order: string[] = [];
for (const k in o) order.push(k);
console.log(order); // [ '1', '2', '3', '10', 'x' ]

// Mixed literal — integer-like keys hoist ahead regardless of literal order.
console.log(Object.keys({ b: 1, 5: 2, a: 3, 1: 4 } as any)); // [ '1', '5', 'b', 'a' ]
console.log(JSON.stringify({ 3: "c", 1: "a", 2: "b" } as any)); // {"1":"a","2":"b","3":"c"}

// No integer keys — insertion order is preserved untouched.
console.log(Object.keys({ banana: 1, apple: 2, cherry: 3 })); // insertion order

// Boundary: 2^32-1 (4294967295) is NOT an array index (it's the length cap),
// so it sorts as a plain string key after real indices.
console.log(Object.keys({ "4294967294": 1, "4294967295": 2, "0": 3 } as any)); // [ '0', '4294967294', '4294967295' ]

// Non-canonical numeric strings ("01" has a leading zero) are plain keys.
console.log(Object.keys({ "01": 1, "0": 2, "1": 3 } as any)); // [ '0', '1', '01' ]

// Empty object.
console.log(Object.keys({} as any)); // []
