import { Writable } from "node:stream";
// new Writable({ signal }) destroys the stream when its AbortController fires.
const ctrl = new AbortController();
const w = new Writable({ signal: ctrl.signal, write(_c, _e, cb) { cb(); } });
let msg = "";
w.on("error", (e) => (msg = (e as Error).name));
ctrl.abort();
setImmediate(() => console.log("abort error name:", msg));
