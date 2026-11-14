// The stream family is linked by `require("stream")` (createRequire): its
// PassThrough pipes as in node. (Perry's `require("stream")` value is the
// namespace object, not the legacy Stream constructor, on main as well: not
// asserted here.)
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const stream = require("stream");

console.log(typeof stream.PassThrough, typeof stream.Readable);
const p = new stream.PassThrough();
const out: string[] = [];
p.on("data", (c: Buffer) => out.push(c.toString()));
p.on("end", () => console.log("passthrough", out.join("|")));
p.write("one");
p.end("two");
