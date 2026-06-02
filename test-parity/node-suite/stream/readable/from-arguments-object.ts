import { Readable } from "node:stream";
// arguments object is iterable (Array-like) — Readable.from yields each arg.
function test() {
  return Readable.from(arguments as any);
}
const r = test("a", "b", "c");
const out: string[] = [];
for await (const v of r) out.push(String(v));
console.log("from arguments:", out.join(","));
