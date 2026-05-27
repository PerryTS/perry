import { crc32 } from "node:zlib";

// crc32(buf2, crc32(buf1)) must equal crc32(concat(buf1, buf2)).
const a = Buffer.from("hello ");
const b = Buffer.from("world");
const combined = Buffer.concat([a, b]);
const oneShot = crc32(combined);
const chained = crc32(b, crc32(a));
console.log("oneShot:", oneShot);
console.log("chained:", chained);
console.log("chain equal:", oneShot === chained);
// Explicit zero seed equals no-seed form for first chunk.
console.log("seed-0 == no-seed:", crc32(a, 0) === crc32(a));
