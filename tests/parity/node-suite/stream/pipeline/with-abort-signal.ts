import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline(...streams, { signal }, cb) aborts when the AbortController fires;
// the callback receives the abort error.
const ctrl = new AbortController();
const src = new Readable({ read() {} });
const sink = new PassThrough();
pipeline(src, sink, { signal: ctrl.signal }, (err) => {
  console.log("err exists:", err !== null && err !== undefined);
});
ctrl.abort();
