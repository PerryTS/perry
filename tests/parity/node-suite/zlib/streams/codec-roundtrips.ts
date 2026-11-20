import * as zlib from "node:zlib";

async function collect(stream: any, input: Buffer) {
  const chunks: Buffer[] = [];
  stream.on("data", (chunk: Buffer) => chunks.push(chunk));
  const settled = new Promise<void>((resolve, reject) => {
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  stream.end(input);
  await settled;
  stream.destroy();
  return Buffer.concat(chunks);
}

const input = Buffer.from("stream codec contract");
for (
  const [name, createCompress, createDecompress] of [
    ["deflate", zlib.createDeflate, zlib.createInflate],
    ["raw", zlib.createDeflateRaw, zlib.createInflateRaw],
    ["gzip", zlib.createGzip, zlib.createGunzip],
    ["unzip", zlib.createGzip, zlib.createUnzip],
    ["brotli", zlib.createBrotliCompress, zlib.createBrotliDecompress],
  ] as const
) {
  const compressed = await collect(createCompress(), input);
  const output = await collect(createDecompress(), compressed);
  console.log(name, output.toString());
}
