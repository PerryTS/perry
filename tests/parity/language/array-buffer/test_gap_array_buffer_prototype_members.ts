// ArrayBuffer and SharedArrayBuffer instances are ordinary objects: their
// members are their own prototype's (node's exact set and order), then
// Object.prototype's. Neither has DataView or %TypedArray% methods.
const keys = (o: object) => Reflect.ownKeys(o).map(String).join(" ");
console.log(keys(ArrayBuffer.prototype));
console.log(keys(SharedArrayBuffer.prototype));
console.log(Object.getPrototypeOf(ArrayBuffer.prototype) === Object.prototype,
  Object.getPrototypeOf(SharedArrayBuffer.prototype) === Object.prototype);

const ab: any = new ArrayBuffer(8);
const sab: any = new SharedArrayBuffer(8);
const names = ["getInt8", "setInt8", "subarray", "set", "fill", "at", "toHex", "entries", "buffer", "byteOffset",
  "length", "slice", "resize", "transfer", "transferToFixedLength", "detached", "resizable", "growable", "grow",
  "maxByteLength", "byteLength", "toString", "valueOf", "toLocaleString"];
for (const [label, o] of [["ab", ab], ["sab", sab]] as [string, any][]) {
  console.log(label, names.map((n) => n + ":" + typeof o[n] + (n in o ? "+" : "-")).join(" "));
}
console.log(ab.slice === ArrayBuffer.prototype.slice, sab.slice === SharedArrayBuffer.prototype.slice,
  ab.valueOf === Object.prototype.valueOf, ab.hasOwnProperty === Object.prototype.hasOwnProperty);

const kind = (f: () => unknown) => {
  try { console.log("ok", f()); } catch (e: any) { console.log(e.constructor.name); }
};
kind(() => ab.getInt8(0));
kind(() => sab.subarray(0));
kind(() => sab.resize(4));

(ArrayBuffer.prototype as any).mk = function () { return 7; };
(SharedArrayBuffer.prototype as any).mk = function (this: SharedArrayBuffer) { return this.byteLength; };
console.log(ab.mk(), sab.mk(), "mk" in ab, typeof sab.mk);
console.log(ab.toString(), sab.toString(), ab.toLocaleString());

// Growable SharedArrayBuffer.
const g: any = new SharedArrayBuffer(4, { maxByteLength: 8 } as any);
console.log(sab.growable, sab.maxByteLength, g.growable, g.maxByteLength, g.byteLength);
const msg = (f: () => unknown) => {
  try { console.log("ok", f()); } catch (e: any) { console.log(e.constructor.name + ": " + e.message); }
};
msg(() => sab.grow(8));
msg(() => g.grow(2));
msg(() => g.grow(9));
msg(() => { g.grow(6); return g.byteLength; });
msg(() => { g.grow(6); return g.byteLength; });
msg(() => new SharedArrayBuffer(9, { maxByteLength: 8 } as any));
const u = new Uint8Array(g);
u[5] = 9;
console.log(u.length, u[5], new Uint8Array(g, 4)[1]);
console.log(SharedArrayBuffer.prototype.grow.length, SharedArrayBuffer.prototype.grow.name);
console.log(typeof Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, "growable")!.get,
  typeof Object.getOwnPropertyDescriptor(SharedArrayBuffer.prototype, "maxByteLength")!.get);
