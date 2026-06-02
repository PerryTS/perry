import { Readable } from "node:stream";
// new Readable({ signal }) destroys the stream when the AbortController fires.
const ctrl = new AbortController();
const r = new Readable({ signal: ctrl.signal, read() {} });
let msg = "";
r.on("error", (e) => (msg = (e as Error).name));
ctrl.abort();
setImmediate(() => console.log("abort error name:", msg));
