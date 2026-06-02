import { Readable } from "node:stream";
// reduce(fn, init, { signal }) rejects on abort.
const ctrl = new AbortController();
const p = Readable.from([1, 2, 3]).reduce(async (a: number, b: number) => a + b, 0, { signal: ctrl.signal });
ctrl.abort();
let msg = "";
try { await p; } catch (e) { msg = (e as Error).name; }
console.log("abort name:", msg);
