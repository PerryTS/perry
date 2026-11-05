import { ReadableStream } from "node:stream/web";
// `new ReadableStream({ type: "bytes" })` constructs a byte stream
// (BYOB reader compatible).
let constructed = false;
try {
  const rs = new ReadableStream({ type: "bytes" } as any);
  constructed = rs instanceof ReadableStream;
  console.log("constructed:", constructed);
  // Try BYOB
  const reader = (rs as any).getReader({ mode: "byob" });
  console.log("byob reader:", typeof reader.read === "function");
} catch (e: any) {
  console.log("threw:", e && e.name);
}
