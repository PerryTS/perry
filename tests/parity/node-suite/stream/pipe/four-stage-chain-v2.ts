import { Readable, Transform, PassThrough } from "node:stream";
// 4-stage pipe: src → t1 → t2 → sink.
const src = Readable.from(["a"]);
const t1 = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "1"); } });
const t2 = new Transform({ transform(c, _e, cb) { cb(null, String(c) + "2"); } });
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
sink.on("end", () => console.log("piped:", out.join(",")));
src.pipe(t1).pipe(t2).pipe(sink);
