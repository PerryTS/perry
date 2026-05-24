import { Readable } from "node:stream";
// Without setEncoding, read() returns Buffer instances.
const r = new Readable({ read() {} });
r.push("hi");
r.push(null);
r.on("readable", () => {
  const got = r.read();
  console.log("is buffer:", Buffer.isBuffer(got));
  console.log("toString:", got && got.toString("utf8"));
});
