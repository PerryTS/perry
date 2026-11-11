// Typed arrays, Buffer and ArrayBuffer spread into calls and literals, unpatched (the patched case is fixtures-gap/spread_typed_array_iter_patched.js).
function f(...a) { return a.join(","); }
const u8 = new Uint8Array([1, 2, 3]);
const f64 = new Float64Array([1.5, -2]);
const buf = Buffer.from([7, 8]);
console.log(f(...u8), f(...f64, 9), f(...buf), [...u8].length, Math.max(...u8), String.fromCharCode(...new Uint8Array([104, 105])));
console.log(f(...u8), f(...buf));
// A computed @@iterator member anywhere makes the iteration facts non-zero: typed arrays and
// Buffers then take the protocol instead of the element copy and must read the same.
const it = { *[Symbol.iterator]() { yield 1; } };
console.log(f(...it, ...u8, ...buf), [...f64, ...buf].join("|"), Math.max(...buf), String.fromCharCode(...new Uint8Array([111, 107])));
function viaArgs() { return f(...arguments); }
console.log(viaArgs(...u8, 4));
