import { Readable } from "node:stream";
// unshift(chunk) pushes a chunk back to the front so the next read returns it.
const r = new Readable({ read() {} });
r.setEncoding("utf8");
r.on("data", (c) => console.log("data:", c));
r.on("end", () => console.log("end"));
r.push("first");
r.unshift("unshifted-");
r.push(null);
