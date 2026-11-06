import { Readable } from "node:stream";
// encoding option in constructor sets readableEncoding.
const r = new Readable({ encoding: "utf8", read() {} });
console.log("encoding:", r.readableEncoding);
r.push(Buffer.from("xyz"));
r.push(null);
r.on("data", (c) => {
  console.log("data is string:", typeof c === "string");
  console.log("value:", c);
});
