import * as fs from "fs";

function sumB(b: Buffer): number {
  let sum = 0;
  for (let i = 0; i < b.length; i++) sum += b[i];
  return sum;
}
function sumU(b: Uint8Array): number {
  let sum = 0;
  for (let i = 0; i < b.length; i++) sum += b[i];
  return sum;
}
function wordB(b: Buffer): number {
  if (b.length < 4) return 0;
  return b.readUInt32BE(0);
}
function show(name: string, b: Buffer) {
  console.log(name, b.length, b[0], sumB(b), sumU(b));
}
function window(name: string, b: Buffer) {
  show(name, b);
  show(name + ".subarray", b.subarray(1));
}

const plain = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
window("plain", plain);
window("subarray4", plain.subarray(4));
window("slice2_5", plain.slice(2, 5));
const ab = new ArrayBuffer(16);
const u8 = new Uint8Array(ab);
for (let i = 0; i < u8.length; i++) u8[i] = i + 1;
window("fromAB", Buffer.from(ab, 3, 5));
window("u8view", new Uint8Array(ab, 6, 4) as unknown as Buffer);
const big = Buffer.alloc(1 << 20);
for (let i = 0; i < big.length; i++) big[i] = (i * 7) & 255;
window("big", big);
window("bigsub1000", big.subarray(1000));
const file = "gap_12094_buffer_param_views.bin";
fs.writeFileSync(file, big);
const read = fs.readFileSync(file);
window("readFile", read);
window("readFileSub5", read.subarray(5));
fs.unlinkSync(file);

console.log("numeric view", wordB(plain.subarray(4)), wordB(Buffer.from(ab, 3, 5)), wordB(read.subarray(5)));
