import { Readable, Transform, PassThrough } from "node:stream";
// r → t1 → t2 → sink — four-stage chain.
const r = Readable.from(["a"]);
const up = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const ex = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "!"); } });
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
sink.on("end", () => console.log("joined:", out.join("")));
r.pipe(up).pipe(ex).pipe(sink);
