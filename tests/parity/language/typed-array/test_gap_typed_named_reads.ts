// Named reads of a typed array's metadata (`length`, `byteLength`,
// `byteOffset`, `buffer`) and its kind prototype's data (`BYTES_PER_ELEMENT`,
// `constructor`) at untyped, typed and field sites, against every receiver
// shape that must not take the structural answer: an own property, a
// subclass, a relinked instance, a detached buffer, a resizable buffer
// (length-tracking and fixed views) and ordinary objects at the same site.
function read(o: any): string {
  const c = o.constructor;
  return [
    o.length,
    o.byteLength,
    o.byteOffset,
    o.BYTES_PER_ELEMENT,
    c === undefined ? "u" : typeof c === "function" ? c.name : String(c),
    o.buffer === undefined ? "nobuf" : o.buffer instanceof ArrayBuffer ? "ab" : typeof o.buffer,
  ].join(" ");
}
function lenTyped(a: Float32Array): number {
  return a.length;
}
class Holder {
  array: any;
  constructor(a: any) {
    this.array = a;
  }
}
const ab = new ArrayBuffer(64);
const kinds: any[] = [
  new Int8Array(3),
  new Uint8Array(3),
  new Uint8ClampedArray(3),
  new Int16Array(3),
  new Uint16Array(3),
  new Int32Array(3),
  new Uint32Array(3),
  new Float32Array(3),
  new Float64Array(3),
  new BigInt64Array(3),
  new BigUint64Array(3),
  new Float32Array(ab, 8, 4),
  new Int16Array(ab, 6),
  new Uint8Array(ab, 3, 5),
];
for (const k of kinds) console.log(read(k), new Holder(k).array.length, read(new Holder(k).array));
console.log("typed", lenTyped(new Float32Array(7)), lenTyped(new Float32Array(ab, 4, 3)));
console.log("typed mismatch", lenTyped(new Int32Array(5) as any));

// Ordinary receivers at the same sites.
console.log(read({ length: 3, byteLength: "b", byteOffset: null, BYTES_PER_ELEMENT: 9, constructor: 1, buffer: 2 }));
console.log(read([1, 2, 3]));
console.log(read("abcd"));
console.log(read(new ArrayBuffer(8)));
console.log(read(new DataView(new ArrayBuffer(8), 2)));

// Own properties shadow the inherited accessors and data.
const own = new Float32Array(4);
Object.defineProperty(own, "length", { value: 99 });
Object.defineProperty(own, "BYTES_PER_ELEMENT", { value: 77 });
Object.defineProperty(own, "constructor", { value: "mine" });
console.log("own", read(own));
const ownView = new Float32Array(ab, 0, 2);
Object.defineProperty(ownView, "byteOffset", { get: () => -1 });
console.log("own view", read(ownView));

// A subclass instance inherits through the subclass prototype.
class Sub extends Float32Array {
  get length(): number {
    return 1234;
  }
}
Object.defineProperty(Sub.prototype, "BYTES_PER_ELEMENT", { value: 3 });
console.log("sub", read(new Sub(2)));
class Plain extends Int16Array {}
console.log("plain sub", read(new Plain(5)));

// A relinked instance.
const relinked = new Float32Array(2);
Object.setPrototypeOf(relinked, Int8Array.prototype);
console.log("relinked", read(relinked));
const toObj = new Uint16Array(2);
Object.setPrototypeOf(toObj, { length: "L", constructor: "C" });
console.log("to object", read(toObj));

// Detached buffers: the owner's alias and a view's owner.
const owner = new Float32Array(4);
console.log("before detach", read(owner));
owner.buffer.transfer();
console.log("detached owner", read(owner));
const dab = new ArrayBuffer(32);
const dv = new Float64Array(dab, 8, 2);
console.log("before view detach", read(dv));
dab.transfer();
console.log("detached view", read(dv));

// A resizable buffer: a length-tracking view and a fixed view.
const rab = new ArrayBuffer(16, { maxByteLength: 64 });
const tracking = new Float32Array(rab);
const fixed = new Float32Array(rab, 4, 2);
console.log("rab", read(tracking), "|", read(fixed));
rab.resize(40);
console.log("grown", read(tracking), "|", read(fixed));
rab.resize(8);
console.log("shrunk", read(tracking), "|", read(fixed));
