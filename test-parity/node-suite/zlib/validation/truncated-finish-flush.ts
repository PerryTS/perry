import * as zlib from "node:zlib";
import { gunzipSync, inflateSync, inflateRawSync } from "node:zlib";

const input = "truncated input contract ".repeat(4);
// Fixed complete streams captured from Node 26.12.1 for this exact input.
// Halving independently compressed streams can stop inside different Huffman tables.
for (
  const [name, hex, decompress] of [
    ["gzip", "1f8b08000000000000132b292acd4b4e2c494d51c8cc2b282d5148cecf2b294a4c2e5128a19e0400bc02291264000000", zlib.gunzipSync],
    ["deflate", "789c2b292acd4b4e2c494d51c8cc2b282d5148cecf2b294a4c2e5128a19e0400bb7926e1", zlib.inflateSync],
    ["raw", "2b292acd4b4e2c494d51c8cc2b282d5148cecf2b294a4c2e5128a19e0400", zlib.inflateRawSync],
  ] as const
) {
  const compressed = Buffer.from(hex, "hex");
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

// Named direct calls complement the captured calls above.
const gz = Buffer.from("1f8b08000000000000132b292acd4b4e2c494d51c8cc2b282d5148cecf2b294a4c2e5128a19e0400bc02291264000000", "hex");
const df = Buffer.from("789c2b292acd4b4e2c494d51c8cc2b282d5148cecf2b294a4c2e5128a19e0400bb7926e1", "hex");
const rw = Buffer.from("2b292acd4b4e2c494d51c8cc2b282d5148cecf2b294a4c2e5128a19e0400", "hex");
const options = { finishFlush: zlib.constants.Z_SYNC_FLUSH };
console.log("direct gzip prefix", input.startsWith(gunzipSync(gz.subarray(0, Math.ceil(gz.length / 2)), options).toString()));
console.log("direct inflate prefix", input.startsWith(inflateSync(df.subarray(0, Math.ceil(df.length / 2)), options).toString()));
console.log("direct raw prefix", input.startsWith(inflateRawSync(rw.subarray(0, Math.ceil(rw.length / 2)), options).toString()));
console.log("complete defaults", gunzipSync(gz).toString() === input, inflateSync(df).toString() === input, inflateRawSync(rw).toString() === input);
for (const [name, decode, bad] of [
  ["gzip", zlib.gunzipSync, Buffer.from([0, 0, 0])],
  ["inflate", zlib.inflateSync, Buffer.from([0, 0, 0])],
  ["raw", zlib.inflateRawSync, Buffer.from([7])],
] as const) {
  try {
    decode(bad, options);
    console.log(name, "corrupt ok");
  } catch (error: any) {
    console.log(name, "corrupt", error.name, error.code);
  }
}
const dictionary = Buffer.from("shared dictionary words");
const withDictionary = zlib.deflateSync("shared dictionary words repeated", { dictionary });
for (const [name, opts] of [["missing", options], ["wrong", { ...options, dictionary: Buffer.from("wrong") }]] as const) {
  try {
    zlib.inflateSync(withDictionary, opts);
    console.log(name, "dictionary ok");
  } catch (error: any) {
    console.log(name, "dictionary", error.name, error.code);
  }
}

// Actual miniz_oxide prefixes can end before the first decoded byte. Both
// engines accept these exact inputs with Z_SYNC_FLUSH and return no output.
for (const [name, hex, decode] of [
  ["gzip", "1f8b0800000000000013adc9b10d00200844d155588d9c1636a721", zlib.gunzipSync],
  ["deflate", "789cadc9b10d00200844d155588d9c1636a7219ffd", zlib.inflateSync],
  ["raw", "adc9b10d00200844d155588d9c1636a7219f", zlib.inflateRawSync],
] as const) {
  const partial = decode(Buffer.from(hex, "hex"), options);
  console.log(name, "empty-output prefix", input.startsWith(partial.toString()), partial.length === 0);
}
