import { Readable, Transform, Writable } from "node:stream";
// Readable → Transform → Writable e2e.
const r = Readable.from(["a", "b"]);
const upper = new Transform({
  transform(c, _e, cb) { cb(null, String(c).toUpperCase()); },
});
const received: string[] = [];
const w = new Writable({
  write(c, _e, cb) { received.push(String(c)); cb(); },
});
r.pipe(upper).pipe(w);
w.on("finish", () => console.log("received:", received.join(",")));
