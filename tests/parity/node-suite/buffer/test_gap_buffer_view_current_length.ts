// A byte view stores its construction length; detach, resize and the
// length-tracking `.buffer` view read the current length from the owner.
function sumMaskedF64(S: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += S[i & 15];
  return s;
}

const ab = new Uint8Array(16).buffer;
console.log("byteLength .buffer:", Buffer.byteLength(ab));
const owner = new ArrayBuffer(64);
const view = new Int32Array(owner, 0, 16);
for (let i = 0; i < 16; i++) view[i] = i + 1;
console.log("masked before:", sumMaskedF64(view, 100));
(owner as any).transfer();
console.log("byteLength detached view:", Buffer.byteLength(view), view.length);
console.log("masked after detach:", sumMaskedF64(view, 100));
const rab = new (ArrayBuffer as any)(16, { maxByteLength: 64 });
const tracking = new Uint8Array(rab);
rab.resize(32);
console.log("tracking after grow:", tracking.length, Buffer.byteLength(tracking));
const fixed = new Uint8Array(rab, 8, 16);
rab.resize(12);
console.log("fixed out of bounds:", fixed.length, Buffer.byteLength(fixed));
