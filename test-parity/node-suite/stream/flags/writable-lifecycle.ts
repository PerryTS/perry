import { Writable } from "node:stream";
// writable.writable is true while the stream accepts writes; flips false
// after end() (Node deprecates writes after end).
const w = new Writable({ write(_c, _e, cb) { cb(); } });
console.log("initial writable:", w.writable);
w.end("done");
w.on("finish", () => console.log("post-finish writable:", w.writable));
w.on("close", () => console.log("post-close writable:", w.writable));
