import { Readable } from "node:stream";
// setEncoding then push(string) — string is delivered as string, not Buffer.
const r = new Readable({ read() {} });
r.setEncoding("utf8");
r.push("hello");
r.push(null);
r.on("data", (c) => {
  console.log("is string:", typeof c === "string");
  console.log("value:", c);
});
