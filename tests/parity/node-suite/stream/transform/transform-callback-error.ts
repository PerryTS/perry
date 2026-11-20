import { Transform } from "node:stream";
// transform(chunk, enc, cb) — calling cb(err) propagates as 'error' event.
const t = new Transform({
  transform(_c, _e, cb) {
    cb(new Error("transform-fail"));
  },
});
t.on("error", (err) => console.log("err:", err && err.message));
t.on("data", () => {});
t.write("x");
