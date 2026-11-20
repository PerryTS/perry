import { Readable, Transform, PassThrough, pipeline } from "node:stream";
// pipeline can wire 3+ streams: source → transform → sink.
const src = Readable.from(["a", "b", "c"]);
const upper = new Transform({
  transform(c, _e, cb) { cb(null, String(c).toUpperCase()); },
});
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
pipeline(src, upper, sink, (err) => {
  console.log("err:", err === null || err === undefined);
  console.log("joined:", out.join(""));
});
