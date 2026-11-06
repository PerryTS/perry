import { Readable, PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";
// stream/promises.pipeline resolves with undefined on clean completion.
const ret = await pipeline(Readable.from(["a"]), new PassThrough());
console.log("ret:", ret);
