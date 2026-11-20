import { ReadableStream } from "node:stream/web";
// ReadableStream.from(non-iterable) — should throw TypeError because
// the argument doesn't have Symbol.iterator or Symbol.asyncIterator.
let caught: string | null = null;
try {
  (ReadableStream as any).from(42);
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
