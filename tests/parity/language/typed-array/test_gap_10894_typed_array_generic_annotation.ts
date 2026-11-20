// #10894 — a receiver ANNOTATED with the generic spelling of a typed array
// (`Uint8Array<ArrayBuffer>`) silently got the wrong `indexOf` / `lastIndexOf`.
//
// TypeScript 5.7 made every typed array, `DataView` and (via @types/node)
// `Buffer` generic over the backing buffer: `new Uint8Array(n)` is now
// `Uint8Array<ArrayBuffer>`, and that is what `tsc` prints and what people
// copy into annotations. The type argument has NO runtime meaning. Perry
// lowered `X<…>` as `Type::Generic { base: "X" }`, while every typed-array /
// buffer recognizer keys on `Type::Named("X")` — so the value read as "known
// not a string, not a typed array" and the Array fast path claimed it:
//
//   function f(m: Uint8Array<ArrayBuffer>) { return m.indexOf(44) }   // -1
//
// (`Float32Array<…>` happened to survive: its `TypedArrayHeader` is in the
// typed-array registry, which the array search helper re-checks at runtime. A
// `Uint8Array` is a `BufferHeader`, which that helper did not recognize.)
//
// Every row below prints VALUES, never a bare truthiness, and the same body
// runs under the bare annotation as a control — so a regression shows as a
// row that differs from its neighbour as well as from node.

const L = (tag: string, key: string, value: unknown): void => {
  console.log(tag + " " + key + "=" + String(value));
};

function show(a: ArrayLike<number> | ArrayLike<bigint>): string {
  return Array.from(a as ArrayLike<number>).join(",");
}

const SRC = [5, 44, 3, 44, 200, 1];
function u8Bare(tag: string, m: Uint8Array): void {
  L(tag, "indexOf", m.indexOf(44));
  L(tag, "indexOfFrom", m.indexOf(44, 2));
  L(tag, "indexOfNeg", m.indexOf(44, -3));
  L(tag, "lastIndexOf", m.lastIndexOf(44));
  L(tag, "lastIndexOfFrom", m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(200));
  L(tag, "includesFrom", m.includes(5, 1));
  L(tag, "absent", m.indexOf(99) + "/" + m.lastIndexOf(99) + "/" + m.includes(99));
  L(tag, "at", m.at(-1) + "/" + m.at(0) + "/" + m.at(9));
  L(tag, "length", m.length + " m[4]=" + m[4]);
  L(tag, "join", m.join("-"));
  L(tag, "slice", show(m.slice(1, 4)));
  L(tag, "subarray", show(m.subarray(2)));
  L(tag, "find", m.find((v) => v > 40) + "/" + m.findIndex((v) => v > 40));
  L(tag, "findLast", m.findLast((v) => v > 40) + "/" + m.findLastIndex((v) => v > 40));
  L(tag, "map", show(m.map((v) => v + 1)));
  L(tag, "filter", show(m.filter((v) => v > 4)));
  L(tag, "reduce", m.reduce((a, v) => a + v, 0) + "/" + m.reduceRight((a, v) => a * 2 + v, 0));
  L(tag, "someEvery", m.some((v) => v === 200) + "/" + m.every((v) => v > 1));
  let fe = 0;
  m.forEach((v, i) => { fe += v * (i + 1); });
  L(tag, "forEach", fe);
  let it = 0;
  for (const v of m) it = it * 3 + v;
  L(tag, "forOf", it);
  L(tag, "spread", [...m].join("|"));
  L(tag, "entries", Array.from(m.entries()).map((e) => e.join(":")).join(","));
  L(tag, "keysValues", Array.from(m.keys()).join(",") + "/" + Array.from(m.values()).join(","));
  L(tag, "copies", show(m.toReversed()) + "/" + show(m.toSorted()) + "/" + show(m.with(0, 9)));
  L(tag, "instanceof", (m instanceof Uint8Array) + " " + Object.prototype.toString.call(m));
  L(tag, "buffer", m.buffer.byteLength + "/" + m.byteOffset + "/" + m.byteLength + "/" + m.BYTES_PER_ELEMENT);
  // In-place mutators, on a copy held in a variable of the same annotation.
  const c: Uint8Array = new Uint8Array(m);
  c.fill(7, 1, 3);
  L(tag, "fill", show(c));
  c.set([1, 2], 4);
  L(tag, "set", show(c));
  c.copyWithin(0, 3);
  L(tag, "copyWithin", show(c));
  c.reverse();
  L(tag, "reverse", show(c));
  c.sort();
  L(tag, "sort", show(c));
  c[0] = 300;
  L(tag, "store", c[0] + "/" + c.indexOf(44) + "/" + c.includes(44));
  L(tag, "source", show(m));
}
function u8ArrayBuffer(tag: string, m: Uint8Array<ArrayBuffer>): void {
  L(tag, "indexOf", m.indexOf(44));
  L(tag, "indexOfFrom", m.indexOf(44, 2));
  L(tag, "indexOfNeg", m.indexOf(44, -3));
  L(tag, "lastIndexOf", m.lastIndexOf(44));
  L(tag, "lastIndexOfFrom", m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(200));
  L(tag, "includesFrom", m.includes(5, 1));
  L(tag, "absent", m.indexOf(99) + "/" + m.lastIndexOf(99) + "/" + m.includes(99));
  L(tag, "at", m.at(-1) + "/" + m.at(0) + "/" + m.at(9));
  L(tag, "length", m.length + " m[4]=" + m[4]);
  L(tag, "join", m.join("-"));
  L(tag, "slice", show(m.slice(1, 4)));
  L(tag, "subarray", show(m.subarray(2)));
  L(tag, "find", m.find((v) => v > 40) + "/" + m.findIndex((v) => v > 40));
  L(tag, "findLast", m.findLast((v) => v > 40) + "/" + m.findLastIndex((v) => v > 40));
  L(tag, "map", show(m.map((v) => v + 1)));
  L(tag, "filter", show(m.filter((v) => v > 4)));
  L(tag, "reduce", m.reduce((a, v) => a + v, 0) + "/" + m.reduceRight((a, v) => a * 2 + v, 0));
  L(tag, "someEvery", m.some((v) => v === 200) + "/" + m.every((v) => v > 1));
  let fe = 0;
  m.forEach((v, i) => { fe += v * (i + 1); });
  L(tag, "forEach", fe);
  let it = 0;
  for (const v of m) it = it * 3 + v;
  L(tag, "forOf", it);
  L(tag, "spread", [...m].join("|"));
  L(tag, "entries", Array.from(m.entries()).map((e) => e.join(":")).join(","));
  L(tag, "keysValues", Array.from(m.keys()).join(",") + "/" + Array.from(m.values()).join(","));
  L(tag, "copies", show(m.toReversed()) + "/" + show(m.toSorted()) + "/" + show(m.with(0, 9)));
  L(tag, "instanceof", (m instanceof Uint8Array) + " " + Object.prototype.toString.call(m));
  L(tag, "buffer", m.buffer.byteLength + "/" + m.byteOffset + "/" + m.byteLength + "/" + m.BYTES_PER_ELEMENT);
  // In-place mutators, on a copy held in a variable of the same annotation.
  const c: Uint8Array<ArrayBuffer> = new Uint8Array(m);
  c.fill(7, 1, 3);
  L(tag, "fill", show(c));
  c.set([1, 2], 4);
  L(tag, "set", show(c));
  c.copyWithin(0, 3);
  L(tag, "copyWithin", show(c));
  c.reverse();
  L(tag, "reverse", show(c));
  c.sort();
  L(tag, "sort", show(c));
  c[0] = 300;
  L(tag, "store", c[0] + "/" + c.indexOf(44) + "/" + c.includes(44));
  L(tag, "source", show(m));
}
function u8ArrayBufferLike(tag: string, m: Uint8Array<ArrayBufferLike>): void {
  L(tag, "indexOf", m.indexOf(44));
  L(tag, "indexOfFrom", m.indexOf(44, 2));
  L(tag, "indexOfNeg", m.indexOf(44, -3));
  L(tag, "lastIndexOf", m.lastIndexOf(44));
  L(tag, "lastIndexOfFrom", m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(200));
  L(tag, "includesFrom", m.includes(5, 1));
  L(tag, "absent", m.indexOf(99) + "/" + m.lastIndexOf(99) + "/" + m.includes(99));
  L(tag, "at", m.at(-1) + "/" + m.at(0) + "/" + m.at(9));
  L(tag, "length", m.length + " m[4]=" + m[4]);
  L(tag, "join", m.join("-"));
  L(tag, "slice", show(m.slice(1, 4)));
  L(tag, "subarray", show(m.subarray(2)));
  L(tag, "find", m.find((v) => v > 40) + "/" + m.findIndex((v) => v > 40));
  L(tag, "findLast", m.findLast((v) => v > 40) + "/" + m.findLastIndex((v) => v > 40));
  L(tag, "map", show(m.map((v) => v + 1)));
  L(tag, "filter", show(m.filter((v) => v > 4)));
  L(tag, "reduce", m.reduce((a, v) => a + v, 0) + "/" + m.reduceRight((a, v) => a * 2 + v, 0));
  L(tag, "someEvery", m.some((v) => v === 200) + "/" + m.every((v) => v > 1));
  let fe = 0;
  m.forEach((v, i) => { fe += v * (i + 1); });
  L(tag, "forEach", fe);
  let it = 0;
  for (const v of m) it = it * 3 + v;
  L(tag, "forOf", it);
  L(tag, "spread", [...m].join("|"));
  L(tag, "entries", Array.from(m.entries()).map((e) => e.join(":")).join(","));
  L(tag, "keysValues", Array.from(m.keys()).join(",") + "/" + Array.from(m.values()).join(","));
  L(tag, "copies", show(m.toReversed()) + "/" + show(m.toSorted()) + "/" + show(m.with(0, 9)));
  L(tag, "instanceof", (m instanceof Uint8Array) + " " + Object.prototype.toString.call(m));
  L(tag, "buffer", m.buffer.byteLength + "/" + m.byteOffset + "/" + m.byteLength + "/" + m.BYTES_PER_ELEMENT);
  // In-place mutators, on a copy held in a variable of the same annotation.
  const c: Uint8Array<ArrayBufferLike> = new Uint8Array(m);
  c.fill(7, 1, 3);
  L(tag, "fill", show(c));
  c.set([1, 2], 4);
  L(tag, "set", show(c));
  c.copyWithin(0, 3);
  L(tag, "copyWithin", show(c));
  c.reverse();
  L(tag, "reverse", show(c));
  c.sort();
  L(tag, "sort", show(c));
  c[0] = 300;
  L(tag, "store", c[0] + "/" + c.indexOf(44) + "/" + c.includes(44));
  L(tag, "source", show(m));
}
function u8Shared(tag: string, m: Uint8Array<SharedArrayBuffer>): void {
  L(tag, "indexOf", m.indexOf(44));
  L(tag, "indexOfFrom", m.indexOf(44, 2));
  L(tag, "indexOfNeg", m.indexOf(44, -3));
  L(tag, "lastIndexOf", m.lastIndexOf(44));
  L(tag, "lastIndexOfFrom", m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(200));
  L(tag, "includesFrom", m.includes(5, 1));
  L(tag, "absent", m.indexOf(99) + "/" + m.lastIndexOf(99) + "/" + m.includes(99));
  L(tag, "at", m.at(-1) + "/" + m.at(0) + "/" + m.at(9));
  L(tag, "length", m.length + " m[4]=" + m[4]);
  L(tag, "join", m.join("-"));
  L(tag, "slice", show(m.slice(1, 4)));
  L(tag, "subarray", show(m.subarray(2)));
  L(tag, "find", m.find((v) => v > 40) + "/" + m.findIndex((v) => v > 40));
  L(tag, "findLast", m.findLast((v) => v > 40) + "/" + m.findLastIndex((v) => v > 40));
  L(tag, "map", show(m.map((v) => v + 1)));
  L(tag, "filter", show(m.filter((v) => v > 4)));
  L(tag, "reduce", m.reduce((a, v) => a + v, 0) + "/" + m.reduceRight((a, v) => a * 2 + v, 0));
  L(tag, "someEvery", m.some((v) => v === 200) + "/" + m.every((v) => v > 1));
  let fe = 0;
  m.forEach((v, i) => { fe += v * (i + 1); });
  L(tag, "forEach", fe);
  let it = 0;
  for (const v of m) it = it * 3 + v;
  L(tag, "forOf", it);
  L(tag, "spread", [...m].join("|"));
  L(tag, "entries", Array.from(m.entries()).map((e) => e.join(":")).join(","));
  L(tag, "keysValues", Array.from(m.keys()).join(",") + "/" + Array.from(m.values()).join(","));
  L(tag, "copies", show(m.toReversed()) + "/" + show(m.toSorted()) + "/" + show(m.with(0, 9)));
  L(tag, "instanceof", (m instanceof Uint8Array) + " " + Object.prototype.toString.call(m));
  L(tag, "buffer", m.buffer.byteLength + "/" + m.byteOffset + "/" + m.byteLength + "/" + m.BYTES_PER_ELEMENT);
  // In-place mutators, on a copy held in a variable of the same annotation.
  const c: Uint8Array<ArrayBuffer> = new Uint8Array(m);
  c.fill(7, 1, 3);
  L(tag, "fill", show(c));
  c.set([1, 2], 4);
  L(tag, "set", show(c));
  c.copyWithin(0, 3);
  L(tag, "copyWithin", show(c));
  c.reverse();
  L(tag, "reverse", show(c));
  c.sort();
  L(tag, "sort", show(c));
  c[0] = 300;
  L(tag, "store", c[0] + "/" + c.indexOf(44) + "/" + c.includes(44));
  L(tag, "source", show(m));
}
function u8Readonly(tag: string, m: Readonly<Uint8Array>): void {
  L(tag, "indexOf", m.indexOf(44));
  L(tag, "indexOfFrom", m.indexOf(44, 2));
  L(tag, "indexOfNeg", m.indexOf(44, -3));
  L(tag, "lastIndexOf", m.lastIndexOf(44));
  L(tag, "lastIndexOfFrom", m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(200));
  L(tag, "includesFrom", m.includes(5, 1));
  L(tag, "absent", m.indexOf(99) + "/" + m.lastIndexOf(99) + "/" + m.includes(99));
  L(tag, "at", m.at(-1) + "/" + m.at(0) + "/" + m.at(9));
  L(tag, "length", m.length + " m[4]=" + m[4]);
  L(tag, "join", m.join("-"));
  L(tag, "slice", show(m.slice(1, 4)));
  L(tag, "subarray", show(m.subarray(2)));
  L(tag, "find", m.find((v) => v > 40) + "/" + m.findIndex((v) => v > 40));
  L(tag, "findLast", m.findLast((v) => v > 40) + "/" + m.findLastIndex((v) => v > 40));
  L(tag, "map", show(m.map((v) => v + 1)));
  L(tag, "filter", show(m.filter((v) => v > 4)));
  L(tag, "reduce", m.reduce((a, v) => a + v, 0) + "/" + m.reduceRight((a, v) => a * 2 + v, 0));
  L(tag, "someEvery", m.some((v) => v === 200) + "/" + m.every((v) => v > 1));
  let fe = 0;
  m.forEach((v, i) => { fe += v * (i + 1); });
  L(tag, "forEach", fe);
  let it = 0;
  for (const v of m) it = it * 3 + v;
  L(tag, "forOf", it);
  L(tag, "spread", [...m].join("|"));
  L(tag, "entries", Array.from(m.entries()).map((e) => e.join(":")).join(","));
  L(tag, "keysValues", Array.from(m.keys()).join(",") + "/" + Array.from(m.values()).join(","));
  L(tag, "copies", show(m.toReversed()) + "/" + show(m.toSorted()) + "/" + show(m.with(0, 9)));
  L(tag, "instanceof", (m instanceof Uint8Array) + " " + Object.prototype.toString.call(m));
  L(tag, "buffer", m.buffer.byteLength + "/" + m.byteOffset + "/" + m.byteLength + "/" + m.BYTES_PER_ELEMENT);
  // In-place mutators, on a copy held in a variable of the same annotation.
  const c: Uint8Array<ArrayBuffer> = new Uint8Array(m);
  c.fill(7, 1, 3);
  L(tag, "fill", show(c));
  c.set([1, 2], 4);
  L(tag, "set", show(c));
  c.copyWithin(0, 3);
  L(tag, "copyWithin", show(c));
  c.reverse();
  L(tag, "reverse", show(c));
  c.sort();
  L(tag, "sort", show(c));
  c[0] = 300;
  L(tag, "store", c[0] + "/" + c.indexOf(44) + "/" + c.includes(44));
  L(tag, "source", show(m));
}
function u8ReadonlyGeneric(tag: string, m: Readonly<Uint8Array<ArrayBuffer>>): void {
  L(tag, "indexOf", m.indexOf(44));
  L(tag, "indexOfFrom", m.indexOf(44, 2));
  L(tag, "indexOfNeg", m.indexOf(44, -3));
  L(tag, "lastIndexOf", m.lastIndexOf(44));
  L(tag, "lastIndexOfFrom", m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(200));
  L(tag, "includesFrom", m.includes(5, 1));
  L(tag, "absent", m.indexOf(99) + "/" + m.lastIndexOf(99) + "/" + m.includes(99));
  L(tag, "at", m.at(-1) + "/" + m.at(0) + "/" + m.at(9));
  L(tag, "length", m.length + " m[4]=" + m[4]);
  L(tag, "join", m.join("-"));
  L(tag, "slice", show(m.slice(1, 4)));
  L(tag, "subarray", show(m.subarray(2)));
  L(tag, "find", m.find((v) => v > 40) + "/" + m.findIndex((v) => v > 40));
  L(tag, "findLast", m.findLast((v) => v > 40) + "/" + m.findLastIndex((v) => v > 40));
  L(tag, "map", show(m.map((v) => v + 1)));
  L(tag, "filter", show(m.filter((v) => v > 4)));
  L(tag, "reduce", m.reduce((a, v) => a + v, 0) + "/" + m.reduceRight((a, v) => a * 2 + v, 0));
  L(tag, "someEvery", m.some((v) => v === 200) + "/" + m.every((v) => v > 1));
  let fe = 0;
  m.forEach((v, i) => { fe += v * (i + 1); });
  L(tag, "forEach", fe);
  let it = 0;
  for (const v of m) it = it * 3 + v;
  L(tag, "forOf", it);
  L(tag, "spread", [...m].join("|"));
  L(tag, "entries", Array.from(m.entries()).map((e) => e.join(":")).join(","));
  L(tag, "keysValues", Array.from(m.keys()).join(",") + "/" + Array.from(m.values()).join(","));
  L(tag, "copies", show(m.toReversed()) + "/" + show(m.toSorted()) + "/" + show(m.with(0, 9)));
  L(tag, "instanceof", (m instanceof Uint8Array) + " " + Object.prototype.toString.call(m));
  L(tag, "buffer", m.buffer.byteLength + "/" + m.byteOffset + "/" + m.byteLength + "/" + m.BYTES_PER_ELEMENT);
  // In-place mutators, on a copy held in a variable of the same annotation.
  const c: Uint8Array<ArrayBuffer> = new Uint8Array(m);
  c.fill(7, 1, 3);
  L(tag, "fill", show(c));
  c.set([1, 2], 4);
  L(tag, "set", show(c));
  c.copyWithin(0, 3);
  L(tag, "copyWithin", show(c));
  c.reverse();
  L(tag, "reverse", show(c));
  c.sort();
  L(tag, "sort", show(c));
  c[0] = 300;
  L(tag, "store", c[0] + "/" + c.indexOf(44) + "/" + c.includes(44));
  L(tag, "source", show(m));
}
u8Bare("Uint8Array", new Uint8Array(SRC));
u8ArrayBuffer("Uint8Array<ArrayBuffer>", new Uint8Array(SRC));
u8ArrayBufferLike("Uint8Array<ArrayBufferLike>", new Uint8Array(SRC));
u8Readonly("Readonly<Uint8Array>", new Uint8Array(SRC));
u8ReadonlyGeneric("Readonly<Uint8Array<ArrayBuffer>>", new Uint8Array(SRC));
{
  const shared: Uint8Array<SharedArrayBuffer> = new Uint8Array(new SharedArrayBuffer(SRC.length));
  shared.set(SRC);
  u8Shared("Uint8Array<SharedArrayBuffer>", shared);
}
function kindInt8Array(tag: string, m: Int8Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Int8Array<ArrayBuffer> = new Int8Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Int8Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindInt8Array("Int8Array<ArrayBuffer>", new Int8Array([5, 44, 3, 44, 100, 1]));
function kindUint8ClampedArray(tag: string, m: Uint8ClampedArray<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Uint8ClampedArray<ArrayBuffer> = new Uint8ClampedArray(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Uint8ClampedArray) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindUint8ClampedArray("Uint8ClampedArray<ArrayBuffer>", new Uint8ClampedArray([5, 44, 3, 44, 100, 1]));
function kindInt16Array(tag: string, m: Int16Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Int16Array<ArrayBuffer> = new Int16Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Int16Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindInt16Array("Int16Array<ArrayBuffer>", new Int16Array([5, 44, 3, 44, 100, 1]));
function kindUint16Array(tag: string, m: Uint16Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Uint16Array<ArrayBuffer> = new Uint16Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Uint16Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindUint16Array("Uint16Array<ArrayBuffer>", new Uint16Array([5, 44, 3, 44, 100, 1]));
function kindInt32Array(tag: string, m: Int32Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Int32Array<ArrayBuffer> = new Int32Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Int32Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindInt32Array("Int32Array<ArrayBuffer>", new Int32Array([5, 44, 3, 44, 100, 1]));
function kindUint32Array(tag: string, m: Uint32Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Uint32Array<ArrayBuffer> = new Uint32Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Uint32Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindUint32Array("Uint32Array<ArrayBuffer>", new Uint32Array([5, 44, 3, 44, 100, 1]));
function kindFloat16Array(tag: string, m: Float16Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Float16Array<ArrayBuffer> = new Float16Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Float16Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindFloat16Array("Float16Array<ArrayBuffer>", new Float16Array([5, 44, 3, 44, 100, 1]));
function kindFloat32Array(tag: string, m: Float32Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Float32Array<ArrayBuffer> = new Float32Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Float32Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindFloat32Array("Float32Array<ArrayBuffer>", new Float32Array([5, 44, 3, 44, 100, 1]));
function kindFloat64Array(tag: string, m: Float64Array<ArrayBuffer>): void {
  L(tag, "search", m.indexOf(44) + "/" + m.indexOf(44, 2) + "/" + m.indexOf(44, -3) + "/" + m.lastIndexOf(44) + "/" + m.lastIndexOf(44, 2));
  L(tag, "includes", m.includes(100) + "/" + m.includes(5, 1) + "/" + m.includes(99));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.subarray(2)) + "/" + show(m.map((v) => v + 1)) + "/" + show(m.filter((v) => v > 4)));
  L(tag, "fold", m.reduce((a, v) => a + v, 0) + "/" + m.findIndex((v) => v > 40) + "/" + m.findLast((v) => v > 40));
  const c: Float64Array<ArrayBuffer> = new Float64Array(m);
  c.fill(7, 1, 3);
  c.reverse();
  c.sort();
  L(tag, "mutate", show(c) + "/" + c.indexOf(44) + "/" + c.lastIndexOf(7));
  L(tag, "identity", (m instanceof Float64Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT + "/" + m.byteLength);
}
kindFloat64Array("Float64Array<ArrayBuffer>", new Float64Array([5, 44, 3, 44, 100, 1]));
function kindBigInt64Array(tag: string, m: BigInt64Array<ArrayBufferLike>): void {
  L(tag, "search", m.indexOf(44n) + "/" + m.indexOf(44n, 2) + "/" + m.lastIndexOf(44n) + "/" + m.lastIndexOf(44n, 2) + "/" + m.includes(100n) + "/" + m.includes(99n));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.map((v) => v + 1n)) + "/" + m.reduce((a, v) => a + v, 0n));
  L(tag, "identity", (m instanceof BigInt64Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT);
}
kindBigInt64Array("BigInt64Array<ArrayBufferLike>", new BigInt64Array([5n, 44n, 3n, 44n, 100n, 1n]));
function kindBigUint64Array(tag: string, m: BigUint64Array<ArrayBufferLike>): void {
  L(tag, "search", m.indexOf(44n) + "/" + m.indexOf(44n, 2) + "/" + m.lastIndexOf(44n) + "/" + m.lastIndexOf(44n, 2) + "/" + m.includes(100n) + "/" + m.includes(99n));
  L(tag, "read", m.length + "/" + m[4] + "/" + m.at(-1) + "/" + m.join("-"));
  L(tag, "derive", show(m.slice(1, 4)) + "/" + show(m.map((v) => v + 1n)) + "/" + m.reduce((a, v) => a + v, 0n));
  L(tag, "identity", (m instanceof BigUint64Array) + " " + Object.prototype.toString.call(m) + " " + m.BYTES_PER_ELEMENT);
}
kindBigUint64Array("BigUint64Array<ArrayBufferLike>", new BigUint64Array([5n, 44n, 3n, 44n, 100n, 1n]));

// ---- Node `Buffer<…>` (@types/node) and `DataView<…>` ----
function nodeBuffer(tag: string, b: Buffer<ArrayBuffer>): void {
  L(tag, "indexOf", b.indexOf("lo") + "/" + b.indexOf(108) + "/" + b.indexOf(108, 3) + "/" + b.lastIndexOf("l") + "/" + b.includes("ell") + "/" + b.includes("xyz"));
  L(tag, "read", b.length + "/" + b[1] + "/" + b.readUInt8(4) + "/" + b.toString("hex") + "/" + b.subarray(1, 3).toString());
  L(tag, "identity", Buffer.isBuffer(b) + " " + (b instanceof Uint8Array));
}
nodeBuffer("Buffer<ArrayBuffer>", Buffer.from("hello"));

function dataView(tag: string, v: DataView<ArrayBuffer>): void {
  v.setUint16(0, 0x1234);
  v.setUint8(2, 44);
  L(tag, "read", v.getUint16(0) + "/" + v.getUint8(2) + "/" + v.getUint16(0, true) + "/" + v.byteLength + "/" + v.byteOffset);
  // (`v instanceof DataView` is left out on purpose: it is `false` in Perry
  // for every receiver, annotated or not — a separate, older gap.)
  L(tag, "identity", Object.prototype.toString.call(v));
}
dataView("DataView<ArrayBuffer>", new DataView(new ArrayBuffer(4)));

// ---- every annotation POSITION ----
type Bytes = Uint8Array<ArrayBuffer>;
interface Packet {
  body: Uint8Array<ArrayBuffer>;
}
class Frame {
  payload: Uint8Array<ArrayBuffer>;
  constructor(payload: Uint8Array<ArrayBuffer>) {
    this.payload = payload;
  }
  comma(from: number): number {
    return this.payload.indexOf(44, from);
  }
  static last(p: Uint8Array<ArrayBuffer>): number {
    return p.lastIndexOf(44);
  }
}
function make(): Uint8Array<ArrayBuffer> {
  return new Uint8Array(SRC);
}
function viaAlias(m: Bytes): number {
  return m.indexOf(44, 2);
}
function viaUnion(m: Uint8Array<ArrayBuffer> | null): number {
  return m === null ? -2 : m.indexOf(44, 2);
}
function viaOptional(m?: Uint8Array<ArrayBuffer>): number {
  return m ? m.lastIndexOf(44) : -2;
}
function viaElement(list: Uint8Array<ArrayBuffer>[]): string {
  return list.map((m) => m.indexOf(44) + ":" + m.includes(200)).join(",");
}
function viaDestructure({ body }: Packet): number {
  return body.indexOf(200);
}
const viaArrow = (m: Uint8Array<ArrayBuffer>): boolean => m.includes(200, 4);
function viaRest(...parts: Uint8Array<ArrayBuffer>[]): number {
  return parts[1].indexOf(3);
}
async function* stream(): AsyncGenerator<Uint8Array<ArrayBuffer>> {
  yield make();
}
function viaInner(m: Uint8Array<ArrayBuffer>): number {
  return m.indexOf(44, 2);
}

const annotated: Uint8Array<ArrayBuffer> = make();
L("position", "variable", annotated.indexOf(44, 2) + "/" + annotated.lastIndexOf(44) + "/" + annotated.includes(200));
L("position", "returnType", make().indexOf(44, 2) + "/" + make().includes(200));
L("position", "alias", viaAlias(make()));
L("position", "union", viaUnion(make()) + "/" + viaUnion(null));
L("position", "optional", viaOptional(make()) + "/" + viaOptional());
L("position", "element", viaElement([make(), new Uint8Array([44])]));
L("position", "destructure", viaDestructure({ body: make() }));
L("position", "arrow", viaArrow(make()));
L("position", "rest", viaRest(make(), make()));
L("position", "field", new Frame(make()).comma(2) + "/" + Frame.last(make()));
L("position", "cast", (make() as Uint8Array<ArrayBuffer>).indexOf(44, 2) + "/" + (make() as unknown as Uint8Array<ArrayBufferLike>).lastIndexOf(44));
L("position", "newTypeArgs", new Uint8Array<ArrayBuffer>(SRC).indexOf(44, 2) + "/" + new Uint8Array<ArrayBuffer>(SRC).includes(200));
const held = new Uint8Array<ArrayBuffer>(SRC);
L("position", "newTypeArgsLocal", held.indexOf(44, 2) + "/" + held.lastIndexOf(44) + "/" + held.includes(200));
const table: Map<string, Uint8Array<ArrayBuffer>> = new Map([["k", make()]]);
L("position", "mapValue", table.get("k")!.indexOf(44, 2));
const record: Record<string, Uint8Array<ArrayBuffer>> = { k: make() };
L("position", "recordValue", record.k.lastIndexOf(44));
const tuple: [number, Uint8Array<ArrayBuffer>] = [1, make()];
L("position", "tuple", tuple[1].indexOf(44, 2));

async function main(): Promise<void> {
  // The issue's exact split: a binding whose type is only INFERRED through
  // the generic was always right; handing that same value to an ANNOTATED
  // parameter was not.
  for await (const m of stream()) {
    L("position", "asyncGenerator", m.indexOf(44, 2) + "/" + viaInner(m));
  }
  const p: Promise<Uint8Array<ArrayBuffer>> = Promise.resolve(make());
  L("position", "promise", (await p).lastIndexOf(44));
}
main();
