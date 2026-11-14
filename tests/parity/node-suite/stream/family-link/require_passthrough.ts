// The stream family is linked by `require("stream")`; the default export is
// the legacy Stream constructor and PassThrough pipes as in node.
const stream = require("stream");

console.log(typeof stream, typeof stream.PassThrough, stream.Stream === stream);
const p = new stream.PassThrough();
const out: string[] = [];
p.on("data", (c: Buffer) => out.push(c.toString()));
p.on("end", () => console.log("passthrough", out.join("|")));
p.write("one");
p.end("two");
