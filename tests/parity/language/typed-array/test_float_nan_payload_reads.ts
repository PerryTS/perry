// Guard: the byte readers that turn stored float bits into JS Numbers keep
// canonicalizing NaN payloads that equal NaN-box tag patterns (Float64Array,
// Float32Array, DataView, Buffer).

function show(label: string, x: any): void {
  console.log(label, typeof x, x === 5, Number.isNaN(x), Object.is(x, NaN), String(x));
}

const F64: Array<[string, number, number]> = [
  ["int32-5", 0x7ffe0000, 0x00000005],
  ["undefined", 0x7ffc0000, 0x00000001],
  ["neg-payload", 0xfffe0000, 0x00000005],
  ["handle", 0xfff90000, 0x00000001],
  ["pointer", 0x7ffd0000, 0x00000000],
  ["string", 0x7fff0000, 0x00000000],
];
const F32: Array<[string, number]> = [
  ["f32-int32", 0x7ff00005],
  ["f32-pointer", 0x7fe80000],
  ["f32-string", 0x7ff80000],
];

for (const [name, hi, lo] of F64) {
  const buf = Buffer.alloc(8);
  buf.writeUInt32LE(lo, 0);
  buf.writeUInt32LE(hi, 4);
  const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + 8);
  show("Float64Array " + name, new Float64Array(ab)[0]);
  show("DataView.getFloat64 " + name, new DataView(ab).getFloat64(0, true));
  show("Buffer.readDoubleLE " + name, buf.readDoubleLE(0));
}
for (const [name, bits] of F32) {
  const buf = Buffer.alloc(4);
  buf.writeUInt32LE(bits, 0);
  const ab = buf.buffer.slice(buf.byteOffset, buf.byteOffset + 4);
  show("Float32Array " + name, new Float32Array(ab)[0]);
  show("DataView.getFloat32 " + name, new DataView(ab).getFloat32(0, true));
  show("Buffer.readFloatLE " + name, buf.readFloatLE(0));
}
