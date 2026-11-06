import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const ret = await pipeline(Readable.from(["a", "b"]), async (_source: AsyncIterable<any>) => "AB");
console.log("ret:", ret);
