import { Writable } from "node:stream";
// uncork() without a prior cork() is a safe no-op.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.uncork(); // no cork — should not error
w.end("x");
w.on("finish", () => console.log("finished normally:", true));
