import { PassThrough } from "node:stream";
import { finished } from "node:stream/promises";
// stream/promises.finished accepts a { signal } option and rejects on abort.
const ctrl = new AbortController();
const p = new PassThrough();
let rejected = false;
const f = finished(p, { signal: ctrl.signal }).catch(() => { rejected = true; });
ctrl.abort();
await f;
console.log("rejected on abort:", rejected);
