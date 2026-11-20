import { Readable } from "node:stream";
// unshift(Buffer) — explicit Buffer input is unshifted to the front.
const r = new Readable({ read() {} });
r.push("world");
r.unshift(Buffer.from("hello "));
r.push(null);
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => console.log("joined:", out.join("|")));
