import { Readable } from "node:stream";
// Readable does NOT have Symbol.iterator — using for-of (sync) should throw.
const r = Readable.from(["a"]);
let caught: string | null = null;
try {
  // @ts-expect-error — intentionally use sync for-of on async-only iterable
  for (const _v of r as any) {
    // unreachable
  }
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
