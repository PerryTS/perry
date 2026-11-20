import { Readable } from "node:stream";
// forEach honors { signal } and rejects on abort.
const ctrl = new AbortController();
const p = Readable.from([1, 2, 3]).forEach(async () => {}, { signal: ctrl.signal });
ctrl.abort();
let msg = "";
try { await p; } catch (e) { msg = (e as Error).name; }
console.log("abort name:", msg);
