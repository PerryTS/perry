import { Writable } from "node:stream";
// end(chunk, encoding, callback) — full 3-arg form: writes chunk with
// encoding, then calls cb after finish.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
let cbFired = false;
w.end("done", "utf8", () => (cbFired = true));
w.on("finish", () => {
  setImmediate(() => console.log("end-cb fired:", cbFired));
});
