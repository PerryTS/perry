import { ReadableStream } from "node:stream/web";
// RS.from(bigint) — bigint is not iterable → TypeError.
let caught: string | null = null;
try {
  (ReadableStream as any).from(42n);
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
