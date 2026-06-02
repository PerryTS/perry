import { Writable, isWritable } from "node:stream";
// isWritable becomes false after end().
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("before end:", isWritable(w));
w.end();
w.on("finish", () => {
  setImmediate(() => console.log("after end:", isWritable(w)));
});
