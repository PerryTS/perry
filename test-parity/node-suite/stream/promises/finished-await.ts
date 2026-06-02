import { PassThrough } from "node:stream";
import { finished } from "node:stream/promises";
// stream/promises.finished(stream) returns a Promise that resolves on clean
// completion.
const p = new PassThrough();
p.end("done");
p.resume();
await finished(p);
console.log("resolved cleanly");
