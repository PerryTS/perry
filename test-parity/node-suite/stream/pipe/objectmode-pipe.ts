import { Readable, Writable } from "node:stream";
// Pipe an objectMode Readable into an objectMode Writable; raw objects flow.
const r = Readable.from([{ id: 1 }, { id: 2 }]);
const received: any[] = [];
const w = new Writable({
  objectMode: true,
  write(c, _e, cb) { received.push(c); cb(); },
});
r.pipe(w);
w.on("finish", () => {
  console.log("count:", received.length);
  console.log("first id:", received[0] && received[0].id);
});
