import { Readable, Transform } from "node:stream";
// readable.compose(transform) is the instance-method form of stream.compose.
const r = Readable.from(["a", "b"]);
const upper = new Transform({
  transform(c, _e, cb) { cb(null, String(c).toUpperCase()); },
});
const composed = (r as any).compose(upper);
console.log("composed-is-stream:", typeof composed.on === "function");
