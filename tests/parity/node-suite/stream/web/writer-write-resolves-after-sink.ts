import { WritableStream } from "node:stream/web";
// writer.write() Promise resolution timing: resolves AFTER the sink's
// write() has processed the chunk.
let written = false;
const ws = new WritableStream({
  async write() {
    await new Promise((resolve) => setTimeout(resolve, 5));
    written = true;
  },
});
const w = ws.getWriter();
const p = w.write("x");
console.log("before await:", written);
await p;
console.log("after await:", written);
console.log("resolved after sink:", written);
