import * as zlib from "node:zlib";

const dictionary = Buffer.from("dictionary bytes");
const input = Buffer.from("dictionary bytes dictionary bytes");
const sources = [
  ["buffer", dictionary],
  ["uint8array", new Uint8Array(dictionary)],
  [
    "dataview",
    new DataView(
      dictionary.buffer,
      dictionary.byteOffset,
      dictionary.byteLength,
    ),
  ],
  [
    "arraybuffer",
    dictionary.buffer.slice(
      dictionary.byteOffset,
      dictionary.byteOffset + dictionary.byteLength,
    ),
  ],
] as const;

for (const [name, source] of sources) {
  const compressed = zlib.deflateSync(input, { dictionary: source as any });
  const output = zlib.inflateSync(compressed, { dictionary: source as any });
  console.log(name, output.toString());
}
