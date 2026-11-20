import { WritableStream } from "node:stream/web";
// On a freshly-created writer, ready is already resolved (no backpressure).
const ws = new WritableStream({ write() {} });
const w = ws.getWriter();
let resolved = false;
w.ready.then(() => (resolved = true));
await new Promise((r) => setImmediate(r));
console.log("ready resolved immediately:", resolved);
