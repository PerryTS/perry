import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline() with a signal that's already aborted before pipeline() runs
// should still fire the callback with an AbortError.
const ctrl = new AbortController();
ctrl.abort();
const src = Readable.from(["x"]);
const sink = new PassThrough();
sink.on("data", () => {});
pipeline(src, sink, { signal: ctrl.signal }, (err: any) => {
  console.log("err present:", !!err);
  console.log("err name:", err && err.name);
});
