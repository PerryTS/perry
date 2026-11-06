import { Buffer } from "node:buffer";

// Signed BE/LE round-trip + cross-read for 64-bit ints. The existing
// `bigint/read-write` test covers a single BE write; this one exercises the
// endian-swap path explicitly so a regression in `readBigInt64LE` is
// localized.
const b = Buffer.alloc(16);
b.writeBigInt64BE(BigInt("-9223372036854775808"), 0);
b.writeBigInt64LE(BigInt("-9223372036854775808"), 8);

console.log("hex:", b.toString("hex"));
console.log("int64 BE:", b.readBigInt64BE(0).toString());
console.log("int64 LE:", b.readBigInt64LE(8).toString());
console.log("int64 cross BE to LE:", b.readBigInt64LE(0).toString());
