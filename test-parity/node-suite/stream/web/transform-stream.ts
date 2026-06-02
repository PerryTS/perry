import { ReadableStream, TransformStream } from "node:stream/web";
// new TransformStream({ transform }) wraps a transform pair (writable+readable).
const upper = new TransformStream({
  transform(chunk, controller) {
    controller.enqueue(String(chunk).toUpperCase());
  },
});
const src = new ReadableStream({
  start(c) {
    c.enqueue("hi");
    c.close();
  },
});
const reader = src.pipeThrough(upper).getReader();
const v = await reader.read();
console.log("value:", v.value);
