import * as zlib from "node:zlib";

const dictionary = Buffer.from("common words for a preset dictionary");
const input = Buffer.from("common words common words");
const compressed = zlib.deflateSync(input, { dictionary });

console.log(
  "with dictionary:",
  zlib.inflateSync(compressed, { dictionary }).toString(),
);
try {
  zlib.inflateSync(compressed);
  console.log("without dictionary: ok");
} catch (error: any) {
  console.log("without dictionary:", error.name, error.code);
}
