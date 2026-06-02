import { ReadableStream } from "node:stream/web";
// pull(controller) is called whenever the consumer needs more data.
let pulls = 0;
const rs = new ReadableStream({
  pull(c: any) {
    pulls++;
    c.enqueue("chunk-" + pulls);
    if (pulls >= 2) c.close();
  },
});
const reader = rs.getReader();
const a = await reader.read();
const b = await reader.read();
const eof = await reader.read();
console.log("a:", a.value, "b:", b.value, "done:", eof.done);
