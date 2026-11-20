import { TransformStream } from "node:stream/web";
// If flush() throws, the readable side errors.
const ts = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(c); },
  flush() { throw new Error("flush-fail"); },
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("a");
await writer.close();
let errMsg: string | null = null;
try {
  // consume to trigger flush
  while (true) {
    const { done } = await reader.read();
    if (done) break;
  }
} catch (e: any) {
  errMsg = e && e.message;
}
console.log("rejected with:", errMsg);
