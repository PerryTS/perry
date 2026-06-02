import { PassThrough } from "node:stream";
import { finished } from "node:stream/promises";
// finished(stream, { signal }) should resolve for an already-ended stream
// instead of waiting for a later abort.
const ctrl = new AbortController();
const p = new PassThrough();
p.end();
p.resume();
await finished(p, { signal: ctrl.signal });
ctrl.abort();
console.log("resolved ended with signal");
