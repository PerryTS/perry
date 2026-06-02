import { compose, Readable, Transform } from "node:stream";
// compose(src, t1, t2) where t2 errors — composite emits error.
const src = Readable.from(["a", "b"]);
const t1 = new Transform({ transform(c, _e, cb) { cb(null, c); } });
const t2 = new Transform({
  transform(_c, _e, cb) { cb(new Error("t2-fail")); },
});
const composed: any = compose(src, t1, t2);
let errMsg: string | null = null;
composed.on("error", (e: any) => (errMsg = e && e.message));
composed.on("data", () => {});
composed.on("close", () => console.log("err:", errMsg));
