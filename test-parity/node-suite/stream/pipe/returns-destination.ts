import { Readable, PassThrough } from "node:stream";
// readable.pipe(dst) returns dst (so you can chain `.pipe(next)`).
const r = Readable.from(["x"]);
const middle = new PassThrough();
const sink = new PassThrough();
const returned = r.pipe(middle);
console.log("returned is middle:", returned === middle);
// Chain pipe
const chained = middle.pipe(sink);
console.log("chained is sink:", chained === sink);
sink.on("data", () => {});
sink.on("end", () => console.log("done"));
