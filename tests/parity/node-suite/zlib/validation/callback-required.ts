import * as zlib from "node:zlib";

const input = Buffer.from("x");

const cases: Array<[string, () => any]> = [
  ["gzip missing", () => (zlib.gzip as any)(input)],
  ["gzip undefined", () => zlib.gzip(input, undefined as any)],
  ["gunzip number", () => zlib.gunzip(zlib.gzipSync(input), 1 as any)],
  ["deflate missing", () => (zlib.deflate as any)(input)],
  ["inflate number", () => zlib.inflate(zlib.deflateSync(input), 1 as any)],
  ["deflateRaw missing", () => (zlib.deflateRaw as any)(input)],
  ["inflateRaw number", () => zlib.inflateRaw(zlib.deflateRawSync(input), 1 as any)],
  ["unzip missing", () => (zlib.unzip as any)(zlib.gzipSync(input))],
  ["brotliCompress missing", () => (zlib.brotliCompress as any)(input)],
  ["brotliDecompress number", () =>
    zlib.brotliDecompress(zlib.brotliCompressSync(input), 1 as any)],
];

for (const [label, fn] of cases) {
  try {
    fn();
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code);
  }
}
