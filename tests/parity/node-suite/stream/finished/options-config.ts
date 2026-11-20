import { PassThrough, finished } from "node:stream";
// finished(stream, { error: false, ... }, cb) lets callers opt out of
// specific completion signals.
const p = new PassThrough();
let fired = false;
finished(p, { error: false, readable: false, writable: false }, () => { fired = true; });
p.end();
setImmediate(() => console.log("cb fired:", fired));
