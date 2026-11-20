// A view (a cell linked to its owner, or to a bag that holds the owner) must
// read and write exactly like an owning array: index get/set, length,
// byteLength, nested subarrays, a detached owner and a bagged view.

function sumU8(b: Uint8Array): number {
  let s = 0;
  for (let i = 0; i < b.length; i++) s += b[i];
  return s;
}
function fillU8(b: Uint8Array, v: number): void {
  for (let i = 0; i < b.length; i++) b[i] = v + i;
}
function sumI32(b: Int32Array): number {
  let s = 0;
  for (let i = 0; i < b.length; i++) s += b[i];
  return s;
}
function fillF64(b: Float64Array): void {
  for (let i = 0; i < b.length; i++) b[i] = i * 0.5;
}
function sumF64(b: Float64Array): number {
  let s = 0;
  for (let i = 0; i < b.length; i++) s += b[i];
  return s;
}
function lenOf(b: Uint8Array): number {
  return b.length;
}
function byteLenOf(b: Uint8Array): number {
  return b.byteLength;
}
function at(b: Uint8Array, i: number): number | undefined {
  return b[i];
}
function put(b: Uint8Array, i: number, v: number): void {
  b[i] = v;
}
function word(b: Buffer): number {
  return b.length >= 4 ? b.readUInt32BE(0) : -1;
}
function untypedLen(b: any): number {
  return b.length;
}
function untypedAt(b: any, i: number): any {
  return b[i];
}
function show(name: string, b: Uint8Array) {
  console.log(name, lenOf(b), byteLenOf(b), b.byteOffset, sumU8(b), at(b, 0), at(b, b.length),
    untypedLen(b), untypedAt(b, 1));
}

// Owner vs view of the same bytes.
const own = new Uint8Array(32);
fillU8(own, 1);
const ab = new ArrayBuffer(32);
const view = new Uint8Array(ab);
fillU8(view, 1);
show("owner", own);
show("view", view);
const off = new Uint8Array(ab, 5, 20);
show("offset view", off);
put(off, 0, 200);
console.log("aliased", view[5], at(view, 5));

// Subarray of a subarray links to the same owner with the offsets added.
const sub = view.subarray(3, 30);
const subsub = sub.subarray(4, 10);
show("subarray", sub);
show("subarray.subarray", subsub);
put(subsub, 1, 77);
console.log("subsub aliases", view[8], subsub.byteOffset, subsub.buffer === ab);

// Wider element kinds over an offset window.
const ab2 = new ArrayBuffer(64);
const i32 = new Int32Array(ab2, 8, 10);
for (let i = 0; i < i32.length; i++) i32[i] = i * 3 - 7;
console.log("i32 view", i32.length, i32.byteLength, i32.byteOffset, sumI32(i32));
const f64 = new Float64Array(ab2, 16, 4);
fillF64(f64);
console.log("f64 view", f64.length, f64.byteLength, sumF64(f64), sumI32(i32));
const f64own = new Float64Array(4);
fillF64(f64own);
console.log("f64 owner", f64own.length, sumF64(f64own));

// A bagged view (a named property) keeps reading and writing its owner.
const bagged = new Uint8Array(ab, 2, 12);
(bagged as any).tag = "named";
const named: any = bagged;
named.p0 = 0; named.p1 = 1; named.p2 = 2; named.p3 = 3; named.p4 = 4;
named.p5 = 5; named.p6 = 6; named.p7 = 7; named.p8 = 8; named.p39 = 39;
delete named.p3;
show("bagged", bagged);
put(bagged, 0, 9);
fillU8(bagged, 50);
console.log("bagged writes", view[2], view[13], (bagged as any).tag, (bagged as any).p39);
// An own `length` on a bagged view shadows the prototype getter.
const shadowed = new Uint8Array(ab, 1, 4);
Object.defineProperty(shadowed, "length", { value: 99 });
console.log("shadowed length", lenOf(shadowed), untypedLen(shadowed), sumU8(new Uint8Array(ab, 1, 4)));

// Buffer views: a named method on a bagged view shadows the builtin.
const bab = new ArrayBuffer(16);
const bv = Buffer.from(bab, 4, 8);
bv[0] = 1; bv[1] = 2; bv[2] = 3; bv[3] = 4;
console.log("buffer view", word(bv), bv.length);
(bv as any).readUInt32BE = () => 1234;
console.log("buffer view shadowed", word(bv), bv.length);

// A detached owner: length/byteLength read 0 and elements read undefined.
const dab = new ArrayBuffer(16);
const dview = new Uint8Array(dab, 4, 8);
const dbag = new Uint8Array(dab, 0, 8);
(dbag as any).x = 1;
fillU8(dview, 3);
console.log("before detach", lenOf(dview), sumU8(dview), lenOf(dbag));
const moved = dab.transfer();
console.log("after detach", lenOf(dview), byteLenOf(dview), dview.byteOffset, sumU8(dview),
  at(dview, 0), untypedLen(dview), lenOf(dbag), at(dbag, 0));
put(dview, 0, 5);
console.log("detached write", at(dview, 0), new Uint8Array(moved)[4], moved.byteLength);

// A length-tracking view over a resizable buffer follows its owner.
const rab = new ArrayBuffer(8, { maxByteLength: 32 });
const tracking = new Uint8Array(rab);
const fixedv = new Uint8Array(rab, 0, 8);
rab.resize(16);
console.log("resizable", lenOf(tracking), lenOf(fixedv));
rab.resize(4);
console.log("shrunk", lenOf(tracking), lenOf(fixedv), at(fixedv, 0));
