import { compose, Readable, Transform } from "node:stream";
// Composite stream fires 'end' after upstream exhausts.
const src = Readable.from(["a", "b"]);
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
const composed: any = compose(src, t);
let endFired = false;
composed.on("end", () => (endFired = true));
composed.on("data", () => {});
setImmediate(() => {
  setImmediate(() => console.log("end fired:", endFired));
});
