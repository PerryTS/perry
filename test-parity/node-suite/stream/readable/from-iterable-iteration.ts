import { Readable } from "node:stream";
// `for await (const chunk of readable)` iterates a Readable to completion.
const r = Readable.from(["one", "two", "three"]);
const out: string[] = [];
for await (const chunk of r) out.push(String(chunk));
console.log("joined:", out.join("|"));
