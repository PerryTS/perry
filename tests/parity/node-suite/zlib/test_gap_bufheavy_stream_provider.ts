// A no-auto build must iterate the same native stream its constructor creates.
import { createGunzip, gzipSync } from "node:zlib";
import { Readable } from "node:stream";

async function* chunks(bytes: Uint8Array) {
  yield bytes.subarray(0, 10);
  yield bytes.subarray(10);
}
async function unpack(bytes: Uint8Array): Promise<[number, string]> {
  const out = Readable.from(chunks(bytes)).pipe(createGunzip());
  let length = 0;
  const pieces: Uint8Array[] = [];
  for await (const chunk of out) {
    length += chunk.length;
    pieces.push(chunk);
  }
  return [length, Buffer.concat(pieces).toString("utf8")];
}
const [length, text] = await unpack(gzipSync(Buffer.from("abc")));
console.log(length, text);
const direct = createGunzip();
direct.end(gzipSync(Buffer.from("xyz")));
let size = 0;
for await (const chunk of direct) size += chunk.length;
console.log("direct", size);
