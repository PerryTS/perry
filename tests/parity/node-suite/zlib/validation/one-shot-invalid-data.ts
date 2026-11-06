import * as zlib from "node:zlib";

const invalidValues: Array<[string, any]> = [
  ["undefined", undefined],
  ["null", null],
  ["number", 0],
  ["boolean", true],
  ["object", {}],
  ["array", []],
  ["symbol", Symbol("s")],
];

const methods: Array<[string, (value: any) => any]> = [
  ["gzipSync", (value) => zlib.gzipSync(value)],
  ["gunzipSync", (value) => zlib.gunzipSync(value)],
  ["deflateSync", (value) => zlib.deflateSync(value)],
  ["inflateSync", (value) => zlib.inflateSync(value)],
  ["deflateRawSync", (value) => zlib.deflateRawSync(value)],
  ["inflateRawSync", (value) => zlib.inflateRawSync(value)],
  ["unzipSync", (value) => zlib.unzipSync(value)],
  ["brotliCompressSync", (value) => zlib.brotliCompressSync(value)],
  ["brotliDecompressSync", (value) => zlib.brotliDecompressSync(value)],
  ["crc32", (value) => zlib.crc32(value)],
];

function errorShape(call: (value: any) => any, value: any) {
  try {
    call(value);
    return "no-throw";
  } catch (error: any) {
    return `${error.name}:${error.code}`;
  }
}

for (const [method, call] of methods) {
  console.log(
    `${method} invalid data:`,
    invalidValues.map(([label, value]) => `${label}=${errorShape(call, value)}`).join(","),
  );
}
