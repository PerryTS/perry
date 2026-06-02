import { ReadableStream } from "node:stream/web";
// On a bytes-typed stream, the controller exposes byobRequest when a BYOB
// reader is pulling. Test it exists at all.
let hasByobRequest: any = null;
const rs = new ReadableStream({
  type: "bytes",
  pull(c) {
    hasByobRequest = (c as any).byobRequest !== undefined;
  },
} as any);
const reader = (rs as any).getReader({ mode: "byob" });
const view = new Uint8Array(8);
try {
  await reader.read(view);
} catch {
  // some impls may reject
}
console.log("byobRequest defined during pull:", hasByobRequest);
