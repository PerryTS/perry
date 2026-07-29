import * as zlib from "node:zlib";

const input = "truncated input contract ".repeat(4);
for (
  const [name, compress, decompress] of [
    ["gzip", zlib.gzipSync, zlib.gunzipSync],
    ["deflate", zlib.deflateSync, zlib.inflateSync],
    ["raw", zlib.deflateRawSync, zlib.inflateRawSync],
  ] as const
) {
  const compressed = compress(input);
  const truncated = compressed.subarray(0, Math.ceil(compressed.length / 2));
  try {
    decompress(truncated);
    console.log(name, "default ok");
  } catch (error: any) {
    console.log(name, "default", error.name, error.code);
  }
  const partial = decompress(truncated, {
    finishFlush: zlib.constants.Z_SYNC_FLUSH,
  });
  console.log(
    name,
    "partial prefix",
    input.startsWith(partial.toString()),
    partial.length > 0,
  );
}
