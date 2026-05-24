import { Writable } from "node:stream";
// cork() then immediately uncork() with no writes — finish fires normally.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
w.cork();
w.uncork();
w.end();
w.on("finish", () => console.log("finished normally:", true));
