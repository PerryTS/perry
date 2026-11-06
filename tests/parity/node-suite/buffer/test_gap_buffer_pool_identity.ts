// Gap test: #1225 Buffer.from(buf) shares .buffer identity with src.
// Node carves Buffer.from(buf) out of a shared 8 KiB pool slab so
// src.buffer === cp.buffer.  Perry models the case the issue calls
// out — copy-from-Buffer — by propagating the source's
// ArrayBuffer-alias onto the new buffer.  Bytes are still a real
// copy; only the `.buffer` identity is shared.

import { Buffer, transcode } from "node:buffer";
import { randomBytes } from "node:crypto";

const src = Buffer.from("abc");
const cp = Buffer.from(src);
console.log("src.buffer === cp.buffer:", src.buffer === cp.buffer);

// Identity must be transitive across chained copies.
const cp2 = Buffer.from(cp);
console.log("cp2.buffer === src.buffer:", cp2.buffer === src.buffer);
console.log("cp2.buffer === cp.buffer:", cp2.buffer === cp.buffer);

// Mutating the copy must NOT touch the source bytes.
const src2 = Buffer.from("xyz");
const cp3 = Buffer.from(src2);
cp3[0] = 0xff;
console.log("src2[0] after cp3 write:", src2[0]);
console.log("cp3[0] after cp3 write:", cp3[0]);

// Length is preserved on the copy.
console.log("cp.length:", cp.length);
console.log("cp.byteLength:", cp.byteLength);

// Uint8Array source must NOT share identity (Node spec-allocates a fresh
// ArrayBuffer for `Buffer.from(uint8Array)`).  Guard against over-aliasing.
const u8 = new Uint8Array([1, 2, 3]);
const fromU8 = Buffer.from(u8);
console.log("Buffer.from(uint8Array) shares with src:",
    fromU8.buffer === u8.buffer);

// Runtime changes affect the next pool, and only Node pooling paths join it.
const savedPoolSize = Buffer.poolSize;
Buffer.poolSize = 1024;
let previous = Buffer.allocUnsafe(128);
let fresh = previous;
for (let n = 0; n < 256; n++) {
    fresh = Buffer.allocUnsafe(128);
    if (fresh.buffer !== previous.buffer) break;
    previous = fresh;
}
const copied = Buffer.from([1, 2, 3]);
const concatenated = Buffer.concat([copied, copied]);
console.log("new pool size:", fresh.buffer.byteLength);
console.log("copy and concat pool:", copied.buffer === fresh.buffer, concatenated.buffer === fresh.buffer);
console.log("pool offsets:", copied.byteOffset % 8, concatenated.byteOffset % 8);
console.log("alloc unpooled:", Buffer.alloc(8).buffer.byteLength);
console.log("unsafe boundary unpooled:", Buffer.allocUnsafe(512).buffer.byteLength);
console.log("unsafeSlow unpooled:", Buffer.allocUnsafeSlow(8).buffer.byteLength);
const slowMethod = Buffer.allocUnsafeSlow;
console.log("unsafeSlow extracted unpooled:", slowMethod(8).buffer.byteLength);
console.log("typed owner unpooled:", new Uint8Array(8).buffer.byteLength);
Buffer.poolSize = 2048;
previous = fresh;
for (let n = 0; n < 256; n++) {
    fresh = Buffer.allocUnsafe(128);
    if (fresh.buffer !== previous.buffer) break;
    previous = fresh;
}
console.log("resized pool:", fresh.buffer.byteLength);
Buffer.poolSize = savedPoolSize;

console.log("transcode unpooled:", transcode(Buffer.from("abc"), "utf8", "latin1").buffer.byteLength);
console.log("copyBytesFrom pooled:", Buffer.copyBytesFrom(new Uint8Array([1, 2])).buffer.byteLength === Buffer.allocUnsafe(2).buffer.byteLength);

const activePool = Buffer.allocUnsafe(1).buffer;
Buffer.poolSize = 4294967295;
console.log("existing pool before capacity validation:", Buffer.allocUnsafe(1).buffer === activePool);
Buffer.poolSize = savedPoolSize;

console.log("randomBytes unpooled:", randomBytes(8).buffer.byteLength);
