import { crc32 } from "node:zlib";

// Node accepts strings (interpreted as UTF-8) as well as Buffers.
console.log("crc32 str 'abc':", crc32("abc"));
console.log("crc32 buf 'abc':", crc32(Buffer.from("abc")));
console.log("crc32 str == buf:", crc32("abc") === crc32(Buffer.from("abc")));
