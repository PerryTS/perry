import { Readable } from "node:stream";
// 'readable' event mode (no 'data' listener) — manually drain via read().
const r = new Readable({ read() {} });
r.push("aa");
r.push("bb");
r.push(null);
const out: string[] = [];
r.on("readable", () => {
  let c;
  while ((c = r.read()) !== null) out.push(String(c));
});
r.on("end", () => console.log("drained:", out.join("|")));
