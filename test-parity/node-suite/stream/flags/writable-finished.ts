import { Writable } from "node:stream";
// writable.writableFinished flips true after end() is called AND all
// pending writes (including the implicit "final") have been flushed.
const w = new Writable({
  write(_c, _e, cb) { cb(); },
});
console.log("before end:", w.writableFinished);
w.end("done");
w.on("finish", () => {
  console.log("on finish, writableFinished:", w.writableFinished);
});
