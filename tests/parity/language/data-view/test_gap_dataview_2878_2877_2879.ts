// #2878 DataView numeric accessors, #2877 ArrayBuffer static/prototype,
// #2879 TypedArray set/copyWithin for non-Uint8Array views.

// ---- #2878: DataView accessors ----
const ab = new ArrayBuffer(8);
const dv = new DataView(ab);
dv.setUint16(0, 0x1234);
dv.setUint16(2, 0x1234, true);
console.log(dv.getUint16(0).toString(16));
console.log(dv.getUint16(2, true).toString(16));
console.log([...new Uint8Array(ab).slice(0, 4)].map((x) => x.toString(16)).join(","));
dv.setInt8(4, -1);
console.log(dv.getUint8(4));
dv.setFloat64(0, 3.14159, true);
console.log(dv.getFloat64(0, true));
dv.setFloat32(0, 1.5);
console.log(dv.getFloat32(0));
dv.setInt32(0, -123456, true);
console.log(dv.getInt32(0, true));
dv.setUint32(0, 0xdeadbeef);
console.log(dv.getUint32(0).toString(16));
console.log(typeof dv.getUint8, typeof dv.setInt32);

// ---- #2877: ArrayBuffer static / prototype ----
const ab2 = new ArrayBuffer(4);
const view = new Uint8Array(ab2);
view.set([1, 2, 3, 4]);
console.log(typeof ArrayBuffer.isView);
console.log(ArrayBuffer.isView(view));
console.log(ArrayBuffer.isView(ab2));
console.log([...new Uint8Array(ab2.slice(1, 3))].join(","));
console.log(ab2.byteLength);

// ---- #2879: TypedArray set / copyWithin ----
const i16 = new Int16Array([1, 2, 3, 4]);
i16.set(new Int16Array([9, 8]), 1);
i16.copyWithin(0, 2, 4);
console.log("Int16Array", Array.from(i16).join(","));

const u8 = new Uint8Array([1, 2, 3, 4]);
u8.set(new Uint8Array([9, 8]), 1);
u8.copyWithin(0, 2, 4);
console.log("Uint8Array", Array.from(u8).join(","));

const f64 = new Float64Array([1, 2, 3, 4]);
f64.set(new Float64Array([9, 8]), 1);
f64.copyWithin(0, 2, 4);
console.log("Float64Array", Array.from(f64).join(","));

const i32 = new Int32Array([10, 20, 30, 40, 50]);
i32.set([100, 200], 2);
console.log("i32set", Array.from(i32).join(","));

const f64b = new Float64Array([1.5, 2.5, 3.5, 4.5]);
f64b.copyWithin(1, 0, 2);
console.log("f64cw", Array.from(f64b).join(","));
