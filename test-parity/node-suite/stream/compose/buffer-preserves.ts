import { compose, Readable, Transform } from "node:stream";
// Buffer chunks survive a compose-pipeline as Buffers (not converted to strings).
const src = Readable.from([Buffer.from("ab")]);
const noop = new Transform({ transform(c, _e, cb) { cb(null, c); } });
const composed: any = compose(src, noop);
composed.on("data", (c: any) => {
  console.log("is Buffer:", Buffer.isBuffer(c));
  console.log("content:", c.toString("utf8"));
});
composed.on("end", () => console.log("done"));
