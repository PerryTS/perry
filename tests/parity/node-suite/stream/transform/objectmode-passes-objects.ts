import { Transform } from "node:stream";
// objectMode: true Transform passes objects directly (no Buffer conversion).
const t = new Transform({
  objectMode: true,
  transform(c, _e, cb) { cb(null, { wrapped: c }); },
});
const out: any[] = [];
t.on("data", (c) => out.push(c));
t.on("end", () => {
  console.log("count:", out.length);
  console.log("first wrapped:", out[0] && out[0].wrapped);
});
t.write({ a: 1 });
t.end();
