import { Readable } from "node:stream";
// Readable.from(non-iterable) — should throw TypeError.
let caught: string | null = null;
try {
  Readable.from(42 as any);
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
