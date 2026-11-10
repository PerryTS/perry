import { Readable } from "node:stream";
// Multiple sequential read() calls drain the buffer in order.
const r = new Readable({ read() {} });
r.push("aa");
r.push("bb");
r.push("cc");
r.push(null);
const out: string[] = [];
r.on("readable", () => {
  let c;
  while ((c = r.read(2)) !== null) out.push(String(c));
});
r.on("end", () => console.log("sequential:", out.join("|")));
