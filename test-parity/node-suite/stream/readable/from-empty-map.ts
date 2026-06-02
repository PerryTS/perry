import { Readable } from "node:stream";
// Readable.from(new Map()) iterates an empty Map → no data, immediate end.
const r = Readable.from(new Map());
const out: any[] = [];
r.on("data", (v) => out.push(v));
r.on("end", () => {
  console.log("count:", out.length);
  console.log("is empty:", out.length === 0);
});
