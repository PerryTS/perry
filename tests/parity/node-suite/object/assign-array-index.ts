// #2439: `Object.assign(targetArray, source)` with an integer-keyed source
// must extend the array's length, treating a canonical array index >= length
// as a new element (Node grows length; Perry previously wrote an inert object
// expando and dropped the element from length/enumeration).
//
// Only the integer-index extension path is exercised here — named-key expandos
// on arrays are a separate, orthogonal gap (reading `arr.foo` back).

// Append a new trailing index.
const a = Object.assign([1, 2], { 2: 3 });
console.log("a:", JSON.stringify(a), "len", a.length);

// Overwrite an in-bounds index without changing length.
const b = Object.assign([1, 2, 3], { 1: 9 });
console.log("b:", JSON.stringify(b), "len", b.length);

// Index past the end leaves a hole (serializes to null per JSON.stringify).
const c = Object.assign([1, 2], { 4: 5 });
console.log("c:", JSON.stringify(c), "len", c.length);

// Multiple index keys in one source.
const d = Object.assign([0], { 1: 1, 2: 2, 3: 3 });
console.log("d:", JSON.stringify(d), "len", d.length);

// Plain-object target is unaffected by the array routing.
const e = Object.assign({ x: 1 }, { y: 2 });
console.log("e:", JSON.stringify(e));

// for-of sees the newly-added element (it is a real array slot).
const f = Object.assign([10, 20], { 2: 30 });
const seen: number[] = [];
for (const v of f) seen.push(v);
console.log("f-iter:", JSON.stringify(seen));
