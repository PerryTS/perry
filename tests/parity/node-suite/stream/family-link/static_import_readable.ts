// The stream family is linked by a static import: a Readable built from the
// named export pushes, reads and ends as in node.
import { Readable, Writable } from "node:stream";

const chunks: string[] = [];
const r = new Readable({ read() {} });
r.push("a");
r.push("b");
r.push(null);
r.on("data", (c: Buffer) => chunks.push(c.toString()));
r.on("end", () => {
  console.log("end", chunks.join(","));
  const w = new Writable({
    write(chunk, _enc, cb) {
      console.log("write", chunk.toString());
      cb();
    },
  });
  w.write("x");
  w.end(() => console.log("finish", Object.getPrototypeOf(Readable.prototype) === Object.getPrototypeOf(Writable.prototype)));
});
