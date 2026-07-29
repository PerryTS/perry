import * as zlib from "node:zlib";

const input = Buffer.from("brotli params contract ".repeat(4));
for (const quality of [0, 6, 11]) {
  const compressed = zlib.brotliCompressSync(input, {
    params: { [zlib.constants.BROTLI_PARAM_QUALITY]: quality },
  });
  console.log(quality, zlib.brotliDecompressSync(compressed).equals(input));
}
