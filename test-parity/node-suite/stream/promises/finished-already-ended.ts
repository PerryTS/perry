import { PassThrough } from "node:stream";
import { finished } from "node:stream/promises";
// finished() resolves immediately for a stream that has already ended.
const p = new PassThrough();
p.end();
p.resume();
await finished(p);
console.log("resolved");
