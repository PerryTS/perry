import { Readable, Transform, PassThrough } from "node:stream";
// r.pipe(t).pipe(sink) chains pipes: source → transform → sink.
const r = Readable.from(["a", "b"]);
const upper = new Transform({
  transform(c, _e, cb) { cb(null, String(c).toUpperCase()); },
});
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
sink.on("end", () => console.log("joined:", out.join("")));
r.pipe(upper).pipe(sink);
