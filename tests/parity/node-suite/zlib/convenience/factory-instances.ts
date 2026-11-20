import * as zlib from "node:zlib";

for (
  const [factoryName, className] of [
    ["createDeflate", "Deflate"],
    ["createDeflateRaw", "DeflateRaw"],
    ["createGzip", "Gzip"],
    ["createGunzip", "Gunzip"],
    ["createInflate", "Inflate"],
    ["createInflateRaw", "InflateRaw"],
    ["createUnzip", "Unzip"],
    ["createBrotliCompress", "BrotliCompress"],
    ["createBrotliDecompress", "BrotliDecompress"],
  ] as const
) {
  const stream = (zlib[factoryName] as any)();
  console.log(factoryName, stream instanceof (zlib[className] as any));
  stream.destroy();
}
