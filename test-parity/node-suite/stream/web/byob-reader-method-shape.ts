import { ReadableStream } from "node:stream/web";
// BYOB reader should have read(view) and releaseLock methods.
const rs = new ReadableStream({ type: "bytes" } as any);
const reader = (rs as any).getReader({ mode: "byob" });
console.log("has read:", typeof reader.read);
console.log("has releaseLock:", typeof reader.releaseLock);
console.log("has closed:", "closed" in reader);
