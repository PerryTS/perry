import { aborted } from "node:util";
// SNAPK4: run-queue entries queued at module scope, still pending when n.ts starts.
const g = globalThis as any;
process.nextTick(() => { g.t = 1; });
setTimeout(() => { g.o = 1; }, 25);
g.iv = setInterval(() => { g.v = 1; }, 40);
setImmediate(() => { g.i = 1; });
const ac = new AbortController();
aborted(ac.signal, ac).then(() => { g.a = 1; });
ac.abort();
export {};
