import * as zlib from "node:zlib";
import { brotliDecompressSync, brotliDecompress } from "node:zlib";

const compressed = zlib.brotliCompressSync(Buffer.alloc(128, 97));

for (const maxOutputLength of [64, 128]) {
  try {
    const output = zlib.brotliDecompressSync(compressed, { maxOutputLength });
    console.log(maxOutputLength, "ok", output.length);
  } catch (error: any) {
    console.log(maxOutputLength, error.name, error.code);
  }
}

await new Promise<void>((resolve) => {
  zlib.brotliDecompress(
    compressed,
    { maxOutputLength: 64 },
    (error, output) => {
      console.log("async", error?.name, error?.code, output === undefined);
      resolve();
    },
  );
});

// Exercise direct imports and captured namespace methods as well as namespace calls.
const originalInput = compressed.toString("hex");
const capturedSync = zlib.brotliDecompressSync;
const syncRoutes: Array<[string, (data: Buffer, options?: any) => Buffer]> = [
  ["named", (data, options) => brotliDecompressSync(data, options)],
  ["namespace", (data, options) => zlib.brotliDecompressSync(data, options)],
  ["captured", capturedSync],
];
for (const [name, decompress] of syncRoutes) {
  for (const cap of [64, 128, undefined]) {
    try {
      const output = decompress(compressed, cap === undefined ? undefined : { maxOutputLength: cap });
      console.log(name, String(cap), "ok", Buffer.isBuffer(output), output.length);
    } catch (error: any) {
      console.log(name, String(cap), error.name, error.code);
    }
  }
}
const capturedAsync = zlib.brotliDecompress;
const asyncRoutes: Array<[string, (data: Buffer, options: any, callback: any) => void]> = [
  ["named", (data, options, callback) => brotliDecompress(data, options, callback)],
  ["namespace", (data, options, callback) => zlib.brotliDecompress(data, options, callback)],
  ["captured", capturedAsync],
];
for (const [name, decompress] of asyncRoutes) {
  for (const cap of [64, 128, undefined]) {
    await new Promise<void>((resolve) => {
      decompress(compressed, cap === undefined ? undefined : { maxOutputLength: cap },
        function (this: any, error: any, output: any) {
          console.log("async", name, String(cap), error?.name, error?.code,
            output === undefined, output?.length, this instanceof zlib.BrotliDecompress);
          resolve();
        });
    });
  }
}
console.log("input unchanged:", compressed.toString("hex") === originalInput);
