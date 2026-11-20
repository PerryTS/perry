import * as zlib from "node:zlib";
import {
  BrotliCompress,
  BrotliDecompress,
  Deflate,
  DeflateRaw,
  Gzip,
  Gunzip,
  Inflate,
  InflateRaw,
  Unzip,
} from "node:zlib";

console.log(
  "namespace constructors:",
  [
    `Deflate:${typeof zlib.Deflate}`,
    `DeflateRaw:${typeof zlib.DeflateRaw}`,
    `Gzip:${typeof zlib.Gzip}`,
    `Gunzip:${typeof zlib.Gunzip}`,
    `Inflate:${typeof zlib.Inflate}`,
    `InflateRaw:${typeof zlib.InflateRaw}`,
    `Unzip:${typeof zlib.Unzip}`,
    `BrotliCompress:${typeof zlib.BrotliCompress}`,
    `BrotliDecompress:${typeof zlib.BrotliDecompress}`,
    `ZstdCompress:${typeof (zlib as any).ZstdCompress}`,
    `ZstdDecompress:${typeof (zlib as any).ZstdDecompress}`,
  ].join(","),
);
console.log(
  "named constructors:",
  [
    typeof Deflate,
    typeof DeflateRaw,
    typeof Gzip,
    typeof Gunzip,
    typeof Inflate,
    typeof InflateRaw,
    typeof Unzip,
    typeof BrotliCompress,
    typeof BrotliDecompress,
  ].join(","),
);
console.log(
  "zstd factories:",
  typeof zlib.createZstdCompress,
  typeof zlib.createZstdDecompress,
);
