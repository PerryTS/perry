// Gap test: #1225 Buffer.from(buf) shares .buffer identity with src.
// Node carves Buffer.from(buf) out of a shared 8 KiB pool slab so
// src.buffer === cp.buffer.  Perry models the case the issue calls
// out — copy-from-Buffer — by propagating the source's
// ArrayBuffer-alias onto the new buffer.  Bytes are still a real
// copy; only the `.buffer` identity is shared.

import { Buffer } from "node:buffer";

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
