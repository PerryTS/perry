import * as zlib from "node:zlib";

const dictionary = Buffer.from(
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
);
const input = Buffer.from(
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
);
const compressed = zlib.brotliCompressSync(input, { dictionary } as any);
console.log(
  "with dictionary:",
  zlib.brotliDecompressSync(compressed, { dictionary } as any).equals(input),
);
try {
  zlib.brotliDecompressSync(compressed);
  console.log("without dictionary: ok");
} catch (error: any) {
  console.log("without dictionary:", error.name, error.code);
}
