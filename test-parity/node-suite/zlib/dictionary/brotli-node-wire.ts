import * as zlib from "node:zlib";

const dictionary = Buffer.from(
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
);
const input = Buffer.from(
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
);
// Independent Node 26.12.1 wires, not a self-roundtrip oracle.
for (const hex of ["1bb300f82500ea98c04a914d0ceb01", "1bb300002400ea98c02ad30cbfbe01"]) {
  const compressed = Buffer.from(hex, "hex");
  try {
    console.log("Node wire:", zlib.brotliDecompressSync(compressed, { dictionary } as any).equals(input));
  } catch (error: any) {
    console.log("Node wire:", error.code);
  }
  try {
    const wrong = Buffer.alloc(dictionary.length, 120);
    console.log("wrong dictionary:", zlib.brotliDecompressSync(compressed, { dictionary: wrong } as any).equals(input));
  } catch (error: any) {
    console.log("wrong dictionary:", error.code);
  }
  for (const options of [undefined, { dictionary: Buffer.alloc(0) }]) {
    try {
      zlib.brotliDecompressSync(compressed, options as any);
      console.log("missing dictionary: ok");
    } catch (error: any) {
      console.log("missing dictionary:", error.code);
    }
  }
}
const normal = zlib.brotliCompressSync(input);
console.log("ordinary:", zlib.brotliDecompressSync(normal).equals(input));
const quality = zlib.constants.BROTLI_PARAM_QUALITY;
const q9 = zlib.brotliCompressSync(input, { params: { [quality]: 9 } });
const q11 = zlib.brotliCompressSync(input, { params: { [quality]: 11 } });
console.log("default quality:", normal.equals(q11));
console.log("explicit quality:", !q9.equals(q11));
for (const params of [undefined, { [quality]: 9 }, { [quality]: 11 }]) {
  const compressed = zlib.brotliCompressSync(input, { dictionary, params } as any);
  console.log("dictionary roundtrip:", zlib.brotliDecompressSync(compressed, { dictionary } as any).equals(input));
  try {
    zlib.brotliDecompressSync(compressed);
    console.log("encoded dictionary required: false");
  } catch (error: any) {
    console.log("encoded dictionary required:", error.code);
  }
}
