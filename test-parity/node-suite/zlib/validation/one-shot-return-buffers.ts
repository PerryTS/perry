import * as zlib from "node:zlib";

const input = Buffer.from("buffer-shape");

const cases: Array<[string, Buffer]> = [
  ["gzip", zlib.gzipSync(input)],
  ["gunzip", zlib.gunzipSync(zlib.gzipSync(input))],
  ["deflate", zlib.deflateSync(input)],
  ["inflate", zlib.inflateSync(zlib.deflateSync(input))],
  ["deflateRaw", zlib.deflateRawSync(input)],
  ["inflateRaw", zlib.inflateRawSync(zlib.deflateRawSync(input))],
  ["unzip-gzip", zlib.unzipSync(zlib.gzipSync(input))],
  ["unzip-deflate", zlib.unzipSync(zlib.deflateSync(input))],
  ["brotliCompress", zlib.brotliCompressSync(input)],
  ["brotliDecompress", zlib.brotliDecompressSync(zlib.brotliCompressSync(input))],
  ["zstdCompress", zlib.zstdCompressSync(input)],
  ["zstdDecompress", zlib.zstdDecompressSync(zlib.zstdCompressSync(input))],
];

for (const [label, value] of cases) {
  console.log(
    `${label}:`,
    Buffer.isBuffer(value),
    value instanceof Uint8Array,
    typeof value.length,
    typeof value[0],
  );
}
