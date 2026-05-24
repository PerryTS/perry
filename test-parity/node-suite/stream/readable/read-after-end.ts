import { Readable } from "node:stream";
// After the stream ends, read() returns null indefinitely.
const r = new Readable({ read() {} });
r.push("hello");
r.push(null);
r.on("end", () => {
  console.log("read 1:", r.read());
  console.log("read 2:", r.read());
  console.log("readableEnded:", r.readableEnded);
});
r.on("data", () => {});
