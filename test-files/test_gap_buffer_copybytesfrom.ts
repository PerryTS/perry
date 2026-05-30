// @covers Buffer.copyBytesFrom(view[, offset[, length]])
// Buffer.copyBytesFrom copies raw bytes out of a TypedArray view into a new
// Buffer. offset/length are measured in elements of the source view.

const u16 = Uint16Array.of(0x1234, 0x5678);
console.log(typeof Buffer.copyBytesFrom);
console.log(Buffer.copyBytesFrom(u16).toString("hex"));

const u8 = Uint8Array.of(1, 2, 3, 4);
console.log(Buffer.copyBytesFrom(u8, 1, 2).toString("hex"));
console.log(Buffer.copyBytesFrom(u8, 2).toString("hex"));

const u32 = Uint32Array.of(0x01020304, 0x05060708);
console.log(Buffer.copyBytesFrom(u32).toString("hex"));
console.log(Buffer.copyBytesFrom(u32, 1).toString("hex"));

// Result is a real, independent Buffer.
const b = Buffer.copyBytesFrom(u8);
console.log(Buffer.isBuffer(b), b.length);

const src = Uint8Array.of(9, 9, 9);
const copy = Buffer.copyBytesFrom(src);
src[0] = 0;
console.log(copy.toString("hex"));
