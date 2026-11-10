import { Readable } from "node:stream";
// Readable.from(iterable) builds a Readable that emits each iterable item.
const r = Readable.from(["a", "b", "c"]);
const out: string[] = [];
r.on("data", (chunk) => out.push(String(chunk)));
r.on("end", () => console.log("joined:", out.join(",")));
