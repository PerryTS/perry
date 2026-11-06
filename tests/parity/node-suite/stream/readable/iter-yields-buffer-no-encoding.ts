import { Readable } from "node:stream";
// Without setEncoding, async iteration over a Readable yields Buffer chunks.
const r = new Readable({ read() {} });
r.push("a");
r.push("b");
r.push(null);
for await (const v of r) {
  console.log("isBuffer:", Buffer.isBuffer(v), "content:", (v as Buffer).toString("utf8"));
}
