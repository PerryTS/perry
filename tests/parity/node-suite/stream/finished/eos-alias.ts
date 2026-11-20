import * as stream from "node:stream";
// `stream.eos` is the historical alias for `stream.finished` (kept for
// compatibility with older code).
console.log("eos:", typeof (stream as any).eos === "function");
