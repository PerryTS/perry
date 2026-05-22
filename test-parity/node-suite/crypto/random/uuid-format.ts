import crypto from "node:crypto";
// randomUUID() is non-deterministic, but its shape is fixed: 36 chars, 4
// dashes, version nibble '4', and a valid RFC-4122 variant nibble.
const u = crypto.randomUUID();
console.log("length:", u.length);
console.log("dashes:", (u.match(/-/g) || []).length);
console.log("version:", u[14]);
console.log("variant:", ["8", "9", "a", "b"].includes(u[19].toLowerCase()));
console.log("hex-and-dashes:", /^[0-9a-f-]+$/.test(u));
