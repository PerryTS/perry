import { compose, Duplex, Readable, Transform } from "node:stream";
// compose(...) returns a Duplex — the composite stream is both Readable
// and Writable from the outside, regardless of the inner components.
const src = Readable.from(["a"]);
const t = new Transform({ transform(c, _e, cb) { cb(null, c); } });
const composed: any = compose(src, t);
console.log("instanceof Duplex:", composed instanceof Duplex);
console.log("has write:", typeof composed.write === "function");
console.log("has read:", typeof composed.read === "function");
