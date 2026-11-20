import { Readable } from "node:stream";
// read(1) on a multibyte UTF-8 chunk returns 1 byte (not 1 character).
const r = new Readable({ read() {} });
r.push("é"); // 2 bytes
r.push(null);
r.on("readable", () => {
  const a = r.read(1);
  const b = r.read(1);
  console.log("a length:", a && a.length);
  console.log("b length:", b && b.length);
});
