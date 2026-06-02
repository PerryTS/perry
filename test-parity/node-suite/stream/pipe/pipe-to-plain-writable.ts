import { Readable, Writable } from "node:stream";
// pipe to a Writable instance directly.
const r = Readable.from(["a", "b"]);
const collected: string[] = [];
const w = new Writable({
  write(c, _e, cb) { collected.push(String(c)); cb(); },
});
r.pipe(w);
w.on("finish", () => console.log("collected:", collected.join(",")));
