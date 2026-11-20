// #3663: `new stream.Readable(...)` via a namespace import must pump too.
import * as stream from "node:stream";
const seen: string[] = [];
const r = new stream.Readable({ read() {} });
const w = new stream.Writable({
  write(c: any, _e: any, cb: any) { seen.push(String(c)); cb(); },
});
w.on("finish", () => console.log("finish:", seen.join("|")));
r.pipe(w);
r.push("x"); r.push("y"); r.push(null);
