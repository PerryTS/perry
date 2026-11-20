import { compose, Readable, Transform } from "node:stream";
// compose(src, t1, t2) — order of transforms preserved.
const src = Readable.from(["a"]);
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const wrap = new Transform({ transform(c, _e, cb) { cb(null, "<" + String(c) + ">"); } });
const composed: any = compose(src, upper, wrap);
composed.on("data", (c: any) => console.log("got:", String(c)));
composed.on("end", () => console.log("done"));
