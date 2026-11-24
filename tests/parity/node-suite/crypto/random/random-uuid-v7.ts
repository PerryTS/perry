// randomUUIDv7() returns an RFC 9562 version-7 UUID whose leading 48 bits are the
// Unix millisecond time of the call, and a fresh value per call.
import crypto from "node:crypto";
const before = Date.now();
const v = crypto.randomUUIDv7();
const after = Date.now();
const ms = Number("0x" + v.slice(0, 8) + v.slice(9, 13));
console.log(typeof v, /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(v));
console.log(ms >= before && ms <= after, crypto.randomUUIDv7() !== v);
