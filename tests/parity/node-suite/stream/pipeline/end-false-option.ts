import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline(..., { end: false }, cb) skips end-propagation to the final sink.
const src = Readable.from(["x"]);
const sink = new PassThrough();
let sinkFinished = false;
sink.on("finish", () => (sinkFinished = true));
pipeline(src, sink, { end: false }, (err) => {
  setImmediate(() =>
    console.log("err:", err === null || err === undefined, "sink-finish:", sinkFinished)
  );
});
