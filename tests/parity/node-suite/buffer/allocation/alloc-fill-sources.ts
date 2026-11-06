import { Buffer } from "node:buffer";

// Deno's node/buffer suite mirrors Node coverage for Buffer.alloc fill sources:
// Buffer and Uint8Array fills must repeat/truncate the full source bytes, not
// just coerce the first byte.
console.log("buffer even:", Buffer.alloc(6, Buffer.from([100, 101])).toString("hex"));
console.log("buffer odd:", Buffer.alloc(7, Buffer.from([100, 101])).toString("hex"));
console.log("uint8 even:", Buffer.alloc(6, new Uint8Array([100, 101]) as any).toString("hex"));
console.log("uint8 trunc:", Buffer.alloc(1, new Uint8Array([100, 101]) as any).toString("hex"));
console.log("hex repeat:", Buffer.alloc(13, "64656e6f", "hex").toString());
console.log("base64 fill:", Buffer.alloc(11, "aGVsbG8gd29ybGQ=", "base64").toString());
// Integer fills: covers the INT32-tagged fast path and bool/undefined/null
// coercions. Pre-fix these fell through to the pointer-coercion arm and
// silently produced a zero-filled buffer.
const code = 65;
console.log("int fill:", Buffer.alloc(5, code).toString("hex"));
console.log("bool fill:", Buffer.alloc(3, true as any).toString("hex"));
console.log("undef fill:", Buffer.alloc(3, undefined as any).toString("hex"));
