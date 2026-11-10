import { TransformStream } from "node:stream/web";
// transform() can be async — returning a Promise that resolves when the
// transform is complete. Pull-side reader awaits accordingly.
const ts = new TransformStream({
  async transform(chunk, ctrl) {
    await new Promise((resolve) => setTimeout(resolve, 5));
    ctrl.enqueue(String(chunk).toUpperCase());
  },
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("ab");
await writer.close();
const { value } = await reader.read();
console.log("got:", value);
