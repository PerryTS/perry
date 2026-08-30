import { Buffer as NodeBuffer } from "node:buffer";

const buf = Buffer.from([1, 2, 3]);
const u8 = new Uint8Array([4, 5]);

console.log("buffer brand:", Buffer.isBuffer(buf));
console.log("buffer is uint8:", buf instanceof Uint8Array);
console.log("uint8 brand:", Buffer.isBuffer(u8));
console.log("uint8 is uint8:", u8 instanceof Uint8Array);
console.log("array buffer brand:", Buffer.isBuffer(new ArrayBuffer(2)));
console.log(
  "data view brand:",
  Buffer.isBuffer(new DataView(new ArrayBuffer(2))),
);
const capturedIsBuffer = NodeBuffer.isBuffer;
console.log("captured brand:", capturedIsBuffer(buf), capturedIsBuffer(u8));
console.log("buffer prototype:", Object.getPrototypeOf(buf) === Buffer.prototype);
console.log(
  "buffer parent prototype:",
  Object.getPrototypeOf(Object.getPrototypeOf(buf)) === Uint8Array.prototype,
);
console.log(
  "prototype link:",
  Object.getPrototypeOf(Buffer.prototype) === Uint8Array.prototype,
);
