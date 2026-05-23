import { Readable } from "node:stream";
// filter(fn, { signal }) aborts when the AbortController fires; the
// returned Readable errors with an AbortError.
const ctrl = new AbortController();
const r = Readable.from([1, 2, 3]).filter(async (n: number) => n > 1, { signal: ctrl.signal });
let msg = "";
ctrl.abort();
try { await r.toArray(); } catch (e) { msg = (e as Error).name; }
console.log("abort name:", msg);
