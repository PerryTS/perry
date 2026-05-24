import { Readable, PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";
// pipeline() from node:stream/promises returns a Promise (no callback form).
const src = Readable.from(["a"]);
const sink = new PassThrough();
sink.on("data", () => {});
const result = pipeline(src, sink);
console.log("is Promise:", typeof (result as any).then === "function");
const v = await result;
console.log("resolved to:", v);
console.log("is undefined:", v === undefined);
