import { Readable } from "node:stream";
// toArray accepts { signal } and rejects with AbortError on abort.
const ctrl = new AbortController();
const p = Readable.from([1, 2, 3]).toArray({ signal: ctrl.signal });
ctrl.abort();
let msg = "";
try { await p; } catch (e) { msg = (e as Error).name; }
console.log("abort name:", msg);
