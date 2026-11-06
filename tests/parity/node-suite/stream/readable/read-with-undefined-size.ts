import { Readable } from "node:stream";
// read(undefined) is equivalent to read() with no args — returns all
// buffered bytes.
const r = new Readable({ read() {} });
r.push("hello");
r.push(null);
r.on("readable", () => {
  const got = r.read(undefined as any);
  console.log("got:", got && got.toString("utf8"));
});
