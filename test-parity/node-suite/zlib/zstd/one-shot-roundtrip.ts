import * as zlib from "node:zlib";
import { promisify } from "node:util";

const input = Buffer.from("perry zstd one-shot coverage");

const compressed = zlib.zstdCompressSync(input);
const decompressed = zlib.zstdDecompressSync(compressed);
console.log("zstd sync:", Buffer.isBuffer(compressed), compressed.length > 0, decompressed.toString());

let callbackLine = "";
let callbackDone!: () => void;
const callbackSettled = new Promise<void>((resolve) => {
  callbackDone = resolve;
});
zlib.zstdCompress(input, (err, out) => {
  callbackLine = [
    "zstd callback compress:",
    err === null ? "null" : err.name,
    Buffer.isBuffer(out),
    zlib.zstdDecompressSync(out).toString(),
  ].join(" ");
  callbackDone();
});
await callbackSettled;
console.log(callbackLine);

const promisifiedCompressed = await promisify(zlib.zstdCompress)(input);
const promisifiedPlain = await promisify(zlib.zstdDecompress)(promisifiedCompressed);
console.log(
  "zstd promisify:",
  Buffer.isBuffer(promisifiedCompressed),
  promisifiedPlain.toString(),
);

for (const [label, fn] of [
  ["zstdCompress missing", () => (zlib.zstdCompress as any)(input)],
  ["zstdDecompress number", () => zlib.zstdDecompress(compressed, 1 as any)],
] as const) {
  try {
    fn();
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code);
  }
}
