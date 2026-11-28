// `new DataView(buffer, offset?, length?)` makes a DataView: its byteLength,
// byteOffset and buffer, and its getters and setters in both byte orders, read and
// write the bytes the typed arrays over the same buffer see, in a plain and an async
// body. On `--target web` it was once a plain object whose members were all undefined.

const ab = new ArrayBuffer(16);
const dv = new DataView(ab);
dv.setInt16(0, -2);
dv.setUint32(2, 0xdeadbeef, true);
dv.setFloat64(8, 1.5);
dv.setInt8(6, -1);
console.log(typeof dv, dv.byteLength, dv.byteOffset, dv.buffer === ab, dv instanceof DataView);
console.log(dv.getInt16(0), dv.getUint16(0), dv.getUint32(2, true), dv.getUint32(2), dv.getFloat64(8), dv.getInt8(6), dv.getUint8(6));
const part = new DataView(ab, 4, 8);
console.log(part.byteLength, part.byteOffset, part.getUint8(0), new DataView(ab, 12).byteLength);
dv.setFloat32(12, 0.5, true);
console.log(dv.getFloat32(12, true), new Uint8Array(ab, 12, 4).join(","), Object.prototype.toString.call(dv));
const u8 = new Uint8Array([1, 2, 3, 4]);
console.log(new DataView(u8.buffer).getUint32(0), new DataView(u8.buffer, 1, 2).getUint16(0, true));
async function main(): Promise<void> {
  const d = new DataView(new ArrayBuffer(4));
  d.setUint16(0, 513);
  console.log("async", d.getUint8(0), d.getUint8(1), d.byteLength);
}
main();
