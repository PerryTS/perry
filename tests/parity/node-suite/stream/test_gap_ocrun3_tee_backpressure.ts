// A tee consumes a transform output through the same pull/backpressure protocol.
const source = new ReadableStream<string>({
  start(controller) {
    controller.enqueue("unauthorized");
    controller.enqueue("end");
    controller.close();
  },
});
const [first, second] = source.pipeThrough(new TransformStream<string, string>()).tee();
for await (const chunk of first) console.log("first", chunk);
for await (const chunk of second) console.log("second", chunk);
console.log("closed");
