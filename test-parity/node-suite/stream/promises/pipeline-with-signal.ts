import { Readable, PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";
// stream/promises.pipeline accepts a { signal } option and rejects when the
// AbortController fires.
const ctrl = new AbortController();
const src = new Readable({ read() {} });
const sink = new PassThrough();
let rejected = false;
const p = pipeline(src, sink, { signal: ctrl.signal }).catch(() => { rejected = true; });
ctrl.abort();
await p;
console.log("rejected on abort:", rejected);
