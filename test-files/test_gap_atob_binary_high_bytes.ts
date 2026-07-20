// `atob` must return a "binary string" — each UTF-16 code unit equals one
// decoded byte (0-255), so `charCodeAt(i)` recovers byte i exactly, including
// bytes >= 128. Perry stored the raw decoded bytes as UTF-8, so high bytes
// (invalid UTF-8 on their own) were mis-decoded by charCodeAt — corrupting any
// binary payload round-tripped through base64 (jose/Auth.js decode a JWE's
// random binary IV/ciphertext/tag this way, which broke session-token
// decryption → login failure).

// Direct decode of bytes fa fb fc c8 80 7f.
const b = atob("+vv8yIB/");
console.log("length:", b.length);
console.log("codes:", Array.from(b, (c) => c.charCodeAt(0)).join(","));

// btoa/atob round-trip over the full 0..255 byte range.
let src = "";
for (let i = 0; i < 256; i++) src += String.fromCharCode(i);
const round = atob(btoa(src));
let ok = round.length === 256;
for (let i = 0; i < 256 && ok; i++) ok = round.charCodeAt(i) === i;
console.log("full 0-255 round-trip:", ok);

// ASCII still decodes correctly (the JSON-header case that always worked).
console.log("ascii:", atob("eyJhIjoxfQ"));

// A byte with the high bit set on its own.
const hi = atob("gA=="); // single byte 0x80
console.log("single high byte:", hi.length, hi.charCodeAt(0));
