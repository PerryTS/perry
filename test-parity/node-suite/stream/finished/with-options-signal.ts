import { PassThrough, finished } from "node:stream";
// finished(stream, options, cb) accepts a signal option (AbortSignal).
const p = new PassThrough();
const ctrl = new AbortController();
finished(p, { signal: ctrl.signal }, (err) => {
  console.log("aborted err exists:", err !== null && err !== undefined);
});
ctrl.abort();
