import { PassThrough } from "node:stream";
// pipe(writable, { end: false }) stops the readable from end()-ing the writable.
const src = new PassThrough();
const sink = new PassThrough();
let finished = false;
sink.on("finish", () => (finished = true));
src.pipe(sink, { end: false });
src.end("x");
setImmediate(() =>
  setImmediate(() => console.log("sink finish (should be false):", finished))
);
