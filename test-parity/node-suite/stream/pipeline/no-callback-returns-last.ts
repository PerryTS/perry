import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline(...streams) called without a callback returns the last stream
// (legacy form) — useful for compose-style usage.
const src = Readable.from(["x"]);
const sink = new PassThrough();
const ret = (pipeline as any)(src, sink);
console.log("returns last:", ret === sink);
