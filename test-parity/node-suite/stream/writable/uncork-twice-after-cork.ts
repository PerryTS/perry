import { Writable } from "node:stream";
// cork(); uncork(); uncork() — second uncork is a safe no-op.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.cork();
w.write("x");
w.uncork();
w.uncork(); // no-op
w.end();
w.on("finish", () => console.log("finished:", true));
