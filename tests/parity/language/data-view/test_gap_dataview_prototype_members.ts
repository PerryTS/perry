// A DataView is an ordinary object: its members are DataView.prototype's
// (node's exact set and order), then Object.prototype's. It has none of the
// Buffer / %TypedArray% methods.
const keys = (o: object) => Reflect.ownKeys(o).map(String).join(" ");
console.log(keys(DataView.prototype));
console.log(Object.getPrototypeOf(DataView.prototype) === Object.prototype);

const ab = new ArrayBuffer(16);
const dv: any = new DataView(ab, 2);
for (const name of ["subarray", "slice", "set", "fill", "at", "toHex", "toBase64", "entries", "length", "map", "join",
  "getInt8", "setFloat64", "getFloat16", "setFloat16", "buffer", "byteOffset", "byteLength", "toString", "valueOf",
  "toLocaleString", "hasOwnProperty"]) {
  console.log(name, typeof dv[name], name in dv, Object.prototype.hasOwnProperty.call(DataView.prototype, name));
}
console.log(dv.getInt8 === DataView.prototype.getInt8, dv.valueOf === Object.prototype.valueOf,
  dv.toString === Object.prototype.toString, dv.buffer === ab, dv.byteOffset, dv.byteLength);

const notFn = (f: () => unknown) => {
  try { f(); console.log("no throw"); } catch (e: any) { console.log(e.constructor.name); }
};
notFn(() => dv.subarray(0));
notFn(() => dv.slice(0));
notFn(() => dv.fill(1));

// A member the program stores on DataView.prototype is every view's member.
(DataView.prototype as any).make = function (this: DataView) { return this.byteLength + 100; };
console.log(dv.make(), "make" in dv, typeof dv.make);
console.log(String(dv), dv.toString(), dv.valueOf() === dv);

// getFloat16 / setFloat16: IEEE binary16, round half to even.
const f = new DataView(new ArrayBuffer(8));
for (const v of [1.5, 65504, 65519.99, 65520, 1e-8, 5.960464477539063e-8, 2.9802322387695312e-8, 2049, 2051,
  -0.00006103515625, 0.1, -0, NaN, Infinity, -Infinity, 1 / 3]) {
  f.setFloat16(0, v);
  f.setFloat16(2, v, true);
  console.log(v, f.getFloat16(0), f.getUint16(0), f.getFloat16(2, true), f.getUint16(2, true), Object.is(f.getFloat16(0), -0));
}
console.log(DataView.prototype.getFloat16.length, DataView.prototype.setFloat16.length, DataView.prototype.getFloat16.name);
