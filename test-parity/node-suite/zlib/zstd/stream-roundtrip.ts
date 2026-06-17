import * as zlib from "node:zlib";

async function collect(stream: any, chunks: Array<Buffer | Uint8Array | string>) {
  const out: Uint8Array[] = [];
  stream.on("data", (chunk: Uint8Array) => out.push(chunk));
  const done = new Promise<void>((resolve, reject) => {
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  for (const chunk of chunks) {
    stream.write(chunk);
  }
  stream.end();
  await done;
  return Buffer.concat(out as any);
}

const input = Buffer.from("perry zstd stream coverage");

const compressed = await collect(zlib.createZstdCompress(), [
  input.subarray(0, 8),
  input.subarray(8),
]);
console.log(
  "zstd stream compress:",
  Buffer.isBuffer(compressed),
  compressed.length > 0,
  zlib.zstdDecompressSync(compressed).toString(),
);

const decompressed = await collect(zlib.createZstdDecompress(), [
  zlib.zstdCompressSync(input),
]);
console.log("zstd stream decompress:", decompressed.toString());

const capturedCreate = zlib.createZstdCompress;
const capturedCompressed = await collect(capturedCreate(), [input]);
console.log(
  "zstd captured factory:",
  Buffer.isBuffer(capturedCompressed),
  zlib.zstdDecompressSync(capturedCompressed).toString(),
);

const ctorCompressed = await collect(new (zlib as any).ZstdCompress(), [input]);
const ctorDecompressed = await collect(new (zlib as any).ZstdDecompress(), [
  zlib.zstdCompressSync(input),
]);
console.log(
  "zstd constructors:",
  Buffer.isBuffer(ctorCompressed),
  zlib.zstdDecompressSync(ctorCompressed).toString(),
  ctorDecompressed.toString(),
);

const compress = zlib.createZstdCompress();
console.log(
  "zstd stream methods:",
  typeof compress.write,
  typeof compress.end,
  typeof compress.on,
  typeof compress.pipe,
);
