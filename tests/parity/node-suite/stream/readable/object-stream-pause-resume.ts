import { Readable } from "node:stream";
// pause()/resume() also work on object-mode streams.
const r = Readable.from([{ a: 1 }, { a: 2 }, { a: 3 }]);
const out: any[] = [];
r.pause();
r.on("data", (c) => out.push(c));
setImmediate(() => r.resume());
r.on("end", () => {
  console.log("count:", out.length);
  console.log("first:", out[0] && out[0].a);
});
