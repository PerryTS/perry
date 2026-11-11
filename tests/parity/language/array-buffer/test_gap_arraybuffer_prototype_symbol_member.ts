// A symbol-keyed member the program adds to ArrayBuffer.prototype,
// SharedArrayBuffer.prototype or DataView.prototype is inherited by every
// instance: `ab[Symbol.iterator]` reads it, and for-of runs it.
// Compared byte-for-byte against `node --experimental-strip-types`.

// Unwritten prototypes: no @@iterator, for-of throws.
const plain: any = new ArrayBuffer(4);
console.log(typeof plain[Symbol.iterator]);
try {
  for (const _ of plain) console.log("unreachable");
} catch (e) {
  console.log(e instanceof TypeError);
}

(ArrayBuffer.prototype as any)[Symbol.iterator] = function* (this: ArrayBuffer) {
  yield this.byteLength;
  yield 7;
};
const ab: any = new ArrayBuffer(4);
console.log(typeof ab[Symbol.iterator], ab[Symbol.iterator] === (ArrayBuffer.prototype as any)[Symbol.iterator]);
const seen: number[] = [];
for (const v of ab) seen.push(v);
console.log(seen.join(","));

const tag = Symbol("tag");
(SharedArrayBuffer.prototype as any)[tag] = "sab";
(DataView.prototype as any)[tag] = "dv";
const sab: any = new SharedArrayBuffer(8);
const dv: any = new DataView(new ArrayBuffer(2));
console.log(sab[tag], dv[tag], ab[tag]);

// An own member still shadows the prototype one.
ab[Symbol.iterator] = function* () { yield "own"; };
for (const v of ab) console.log(v);

// Typed arrays keep their built-in iterator.
console.log([...new Uint8Array([1, 2])].join(","));
