import { Readable } from "node:stream";
// Common pattern: drain inside 'readable' listener via while-loop of read().
const r = new Readable({ read() {} });
r.push("aa");
r.push("bb");
r.push("cc");
r.push(null);
const out: string[] = [];
r.on("readable", () => {
  let c;
  while ((c = r.read()) !== null) out.push(String(c));
});
r.on("end", () => console.log("drained:", out.join("|")));
