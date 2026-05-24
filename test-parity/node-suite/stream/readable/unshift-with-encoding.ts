import { Readable } from "node:stream";
// unshift(chunk, encoding) accepts an encoding hint when chunk is a string.
const r = new Readable({ read() {} });
r.setEncoding("utf8");
r.push("world");
r.push(null);
r.unshift("hello ", "utf8");
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => console.log("unshifted+pushed:", out.join("|")));
