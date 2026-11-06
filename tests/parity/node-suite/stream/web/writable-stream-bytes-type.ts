import { WritableStream } from "node:stream/web";
// type:"bytes" is not a documented WritableStream sink option (only on
// ReadableStream). Confirm Node either ignores it or constructs anyway.
let constructed = false;
try {
  const ws = new WritableStream({ type: "bytes", write() {} } as any);
  constructed = ws instanceof WritableStream;
} catch (e: any) {
  console.log("threw:", e && e.name);
}
console.log("constructed (or threw above):", constructed);
