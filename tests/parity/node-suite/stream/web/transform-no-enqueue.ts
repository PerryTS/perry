import { TransformStream } from "node:stream/web";
// TransformStream whose transform() doesn't enqueue — readable is empty.
const ts = new TransformStream({
  transform() {}, // discards everything
});
const writer = ts.writable.getWriter();
const reader = ts.readable.getReader();
await writer.write("a");
await writer.write("b");
await writer.close();
let count = 0;
while (true) {
  const { done } = await reader.read();
  if (done) break;
  count++;
}
console.log("yielded count:", count);
