import * as zlib from "node:zlib";

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
