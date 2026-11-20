import { Writable } from "node:stream";
// After 'finish', destroyed should be true (autoDestroy default).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.on("finish", () => {
  setImmediate(() => {
    console.log("destroyed after finish:", w.destroyed);
  });
});
w.end("done");
