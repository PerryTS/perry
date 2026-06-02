import { ReadableStream } from "node:stream/web";
// Cancel during an active pull — pull is interrupted; further reads see done.
const rs = new ReadableStream({
  async pull(c) {
    await new Promise((resolve) => setTimeout(resolve, 30));
    c.enqueue("x");
  },
});
const reader = rs.getReader();
const readPromise = reader.read();
// Cancel before pull resolves
setTimeout(() => reader.cancel("interrupt"), 5);
const result = await readPromise;
console.log("done:", result.done);
