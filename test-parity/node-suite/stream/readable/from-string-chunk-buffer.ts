import { Readable } from "node:stream";
// Readable.from(string) — single chunk; check whether it's string or Buffer.
const r = Readable.from("hello");
r.on("data", (c) => {
  console.log("type:", typeof c);
  console.log("isBuffer:", Buffer.isBuffer(c));
  console.log("value:", c.toString ? c.toString() : c);
});
