import { crc32 } from "node:zlib";

// Single-byte vectors (low bytes 0x00..0x0F).
for (let i = 0; i <= 0x0f; i++) {
  console.log("crc32 [" + i + "]:", crc32(Buffer.from([i])));
}
// 256-byte identity buffer — well-known test vector for IEEE CRC32.
const ramp = Buffer.alloc(256);
for (let i = 0; i < 256; i++) ramp[i] = i;
console.log("crc32 identity-256:", crc32(ramp));
// 4 KB zero buffer.
console.log("crc32 zero-4096:", crc32(Buffer.alloc(4096)));
