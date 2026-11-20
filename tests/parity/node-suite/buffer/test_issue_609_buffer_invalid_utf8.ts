// Regression test for issue #609:
// `Buffer.toString()` (default 'utf8' encoding) on bytes that aren't
// valid UTF-8 used to SIGSEGV — `compute_utf16_len` ran
// `str::from_utf8_unchecked` (UB on non-UTF-8) and `.encode_utf16().count()`
// walked past the slice end into unmapped memory.
//
// Surface in the wild: @perryts/mysql 0.1.3's encodeParam falls through
// to `Buffer.from(String(buf), 'utf8')` for Buffer parameters (because
// `typeof buf.readUInt8 === 'function'` returns false in Perry, so its
// `isBufferLike` duck-test fails). On a fresh DB, the first parameterized
// INSERT carrying a `Buffer` (random bytes from `crypto.randomBytes`)
// crashed before the row could be written.
//
// Fix: route `Buffer.toString('utf8')` through `String::from_utf8_lossy`
// so invalid sequences are replaced with U+FFFD, matching Node's
// documented `Buffer.toString` semantics. Also harden `compute_utf16_len`
// to fall back to a byte-walking WTF-8 counter when the input isn't
// valid UTF-8 (defense in depth — any future runtime caller that hands
// `js_string_from_bytes` invalid UTF-8 no longer triggers UB).

import * as crypto from "crypto";

let total = 0;
for (let trial = 0; trial < 50; trial++) {
  // Random 64 bytes — overwhelmingly invalid UTF-8 (pre-fix segfault rate
  // was ~50% on this length / shape).
  const dek = crypto.randomBytes(32);
  const mac = crypto.createHmac("sha256", crypto.randomBytes(32)).update(dek).digest();
  const wrapped = Buffer.concat([dek, mac]);
  const s = wrapped.toString();
  total += s.length;
}
console.log("trials done, accumulated str length>0:", total > 0);

// Specific invalid-UTF-8 byte patterns — deterministic, always invalid.
const invalid = Buffer.from([0xC3, 0x28, 0xA0, 0xA1]);
console.log("invalid len bytes:", invalid.length, "str.length:", invalid.toString().length);

const overlong = Buffer.from([0xFE, 0xC0, 0x80, 0x00]);
console.log("overlong len bytes:", overlong.length, "str.length:", overlong.toString().length);

// Round-trip an ASCII buffer — must still match the source bytes.
const ascii = Buffer.from("hello world", "utf8");
console.log("ascii roundtrip:", ascii.toString() === "hello world");

// `Buffer.toString` with explicit 'utf8' takes the same path.
const wrap2 = crypto.randomBytes(16);
console.log("explicit utf8:", typeof wrap2.toString("utf8"));
