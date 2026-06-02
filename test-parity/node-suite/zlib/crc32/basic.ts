import { crc32 } from "node:zlib";

// Well-known IEEE 802.3 CRC32 reference values.
console.log("crc32 empty:", crc32(Buffer.from("")));
console.log("crc32 'a':", crc32(Buffer.from("a")));
console.log("crc32 'abc':", crc32(Buffer.from("abc")));
console.log("crc32 'hello':", crc32(Buffer.from("hello")));
console.log("crc32 'The quick brown fox jumps over the lazy dog':",
  crc32(Buffer.from("The quick brown fox jumps over the lazy dog")));
