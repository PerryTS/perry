import { PassThrough } from "node:stream";
// Stream constructors may be called without `new` (legacy behavior) — they
// auto-instantiate.
const p = (PassThrough as any)();
console.log("is PassThrough:", p instanceof PassThrough);
