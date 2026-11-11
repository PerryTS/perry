import { compose, Readable, Transform, PassThrough } from "node:stream";
// compose(src, t1, PassThrough, t2) — PassThrough in the middle.
const src = Readable.from(["a"]);
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const pt = new PassThrough();
const wrap = new Transform({ transform(c, _e, cb) { cb(null, "<" + String(c) + ">"); } });
const composed: any = compose(src, upper, pt, wrap);
composed.on("data", (c: any) => console.log("got:", String(c)));
composed.on("end", () => console.log("done"));
