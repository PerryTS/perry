import { Readable, Writable } from "node:stream";
// readable.pipe(writable) connects the two so chunks flow + the writable
// receives 'finish' after the readable ends.
const r = Readable.from(["a", "b", "c"]);
const seen: string[] = [];
const w = new Writable({
  write(c, _e, cb) { seen.push(String(c)); cb(); },
});
w.on("finish", () => console.log("joined:", seen.join("")));
r.pipe(w);
