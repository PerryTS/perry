import { TransformStream } from "node:stream/web";
// flush() that returns a Promise — readable.close awaits it.
let flushDone = false;
const ts = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(c); },
  async flush() {
    await new Promise((resolve) => setTimeout(resolve, 5));
    flushDone = true;
  },
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("a");
await writer.close();
// drain readable
while (true) {
  const { done } = await reader.read();
  if (done) break;
}
console.log("flush ran:", flushDone);
