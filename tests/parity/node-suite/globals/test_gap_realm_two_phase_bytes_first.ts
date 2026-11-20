// Two-phase realm: the first touch of the realm is a class used as a value
// (the core). The byte-cell families (typed arrays, %TypedArray%, the
// buffers, DataView) are then reached through instances, before their
// globals are named.
class Core { static n = 1; }
const first = [Core];
console.log("core", first[0].n);
const u = new Uint8Array([1, 2, 3]);
const anyU: any = u;
console.log("typed dynamic", anyU["length"], anyU["map"]((x: number) => x * 2).join(","), typeof anyU.subarray);
console.log("typed tag", Object.prototype.toString.call(u), String(u));
console.log("typed toString", anyU["toString"] === Array.prototype.toString);
console.log("typed proto", Object.getPrototypeOf(u) === Uint8Array.prototype, anyU.constructor === Uint8Array);
const TA: any = Object.getPrototypeOf(Uint8Array);
console.log("%TypedArray%", TA === Object.getPrototypeOf(Float64Array), TA.name, Object.getPrototypeOf(Uint8Array.prototype) === TA.prototype, typeof TA.prototype.map);
console.log("bytes", Uint8Array.BYTES_PER_ELEMENT, Float64Array.prototype.BYTES_PER_ELEMENT, Int16Array.name);
const ab = new ArrayBuffer(8);
const dv = new DataView(ab);
dv.setInt16(0, -2);
console.log("buffer", ab.byteLength, (ab as any).constructor === ArrayBuffer, dv.getInt16(0), Object.prototype.toString.call(dv), Object.getPrototypeOf(dv) === DataView.prototype);
const sab = new SharedArrayBuffer(4);
console.log("shared", sab.byteLength, Object.prototype.toString.call(sab));
const enc = new TextEncoder().encode("hé");
console.log("encoder", enc.length, enc instanceof Uint8Array, (enc as any).constructor === Uint8Array, new TextDecoder().decode(enc));
console.log("clone", structuredClone(new Uint16Array([7, 8])).join(","), atob(btoa("xy")));
const g: any = globalThis;
console.log("identity", Uint8Array === g.Uint8Array, ArrayBuffer === g.ArrayBuffer, DataView === g.DataView, Float32Array === g["Float32Array"]);
console.log("descriptor", JSON.stringify(Object.getOwnPropertyDescriptor(g, "Int32Array"), (k, v) => (typeof v === "function" ? "fn" : v)));
