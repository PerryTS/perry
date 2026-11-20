import { Readable } from "node:stream";
// read(NaN) — Node coerces NaN to the default behavior (returns all buffered).
const r = new Readable({ read() {} });
r.push("abc");
r.push(null);
r.on("readable", () => {
  const got = r.read(NaN as any);
  console.log("got:", got && got.toString("utf8"));
  console.log("got typeof:", typeof got);
});
