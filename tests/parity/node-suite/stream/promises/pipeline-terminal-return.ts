import { Readable, PassThrough } from "node:stream";
import { pipeline } from "node:stream/promises";

const terminal = await pipeline(Readable.from(["a", "b"]), async (source: AsyncIterable<any>) => {
  let out = "";
  for await (const chunk of source) out += String(chunk);
  return out.toUpperCase();
});

const streamToStream = await pipeline(Readable.from(["x"]), new PassThrough());

console.log("terminal return:", terminal);
console.log("stream return undefined:", streamToStream === undefined);
