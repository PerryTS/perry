// Issue #584: TextEncoder().encode() Uint8Array had wrong backing bytes.
// Pre-fix: js_text_encoder_encode_llvm allocated an ArrayHeader with f64
// elements (one f64 per byte), but bytes[i] / for…of read packed u8 from
// offset 8 — yielding the IEEE-754 representation of the first byte
// instead of the byte itself.
const bytes = new TextEncoder().encode("/test/hello");

console.log("byteLength:", bytes.byteLength);
console.log("bytes[0..3]:", bytes[0], bytes[1], bytes[2], bytes[3]);

const collected: number[] = [];
for (const b of bytes) collected.push(b);
console.log("for…of:", collected.join(","));

// Baseline: literal Uint8Array (per #578)
const lit = new Uint8Array([47, 116, 101, 115, 116, 47, 104, 101, 108, 108, 111]);
const litCollected: number[] = [];
for (const b of lit) litCollected.push(b);
console.log("baseline:", litCollected.join(","));

// Multi-byte UTF-8
const enc = new TextEncoder();
const accent = enc.encode("héllo");
console.log("multibyte first 3:", accent[0], accent[1], accent[2]);
console.log("multibyte length:", accent.length);

// Empty string
console.log("empty length:", enc.encode("").length);

// Round-trip
const original = "Hello, World! héèê";
const dec = new TextDecoder();
console.log("round-trip:", dec.decode(enc.encode(original)) === original);
