import { Readable } from "node:stream";
// When the requested size exceeds the buffered bytes (and end is reached),
// read(N) returns whatever is buffered before EOF.
const r = new Readable({ read() {} });
r.push("hi");
r.push(null);
r.on("readable", () => {
  const got = r.read(1000); // request way more than 2 bytes
  console.log("got bytes:", got && got.length);
  console.log("got string:", got && got.toString("utf8"));
});
