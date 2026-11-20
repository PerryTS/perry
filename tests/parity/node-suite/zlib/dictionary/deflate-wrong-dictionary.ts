import * as zlib from "node:zlib";

const dictionary = Buffer.from("common words for a preset dictionary");
const input = Buffer.from("common words common words");
const compressed = zlib.deflateSync(input, { dictionary });
for (const [label, options] of [
  ["missing", undefined],
  ["wrong", { dictionary: Buffer.from("a different preset dictionary") }],
] as const) {
  try {
    zlib.inflateSync(compressed, options);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code, error.message);
  }
}
const padded = Buffer.concat([Buffer.from("prefix"), dictionary, Buffer.from("suffix")]);
const view = new DataView(padded.buffer, padded.byteOffset + 6, dictionary.length);
console.log("sliced view:", zlib.inflateSync(compressed, { dictionary: view }).equals(input));
const stream = zlib.createDeflate({ dictionary });
stream.reset();
const chunks: Buffer[] = [];
stream.on("data", (chunk: Buffer) => chunks.push(chunk));
await new Promise<void>((resolve, reject) => {
  stream.on("end", resolve);
  stream.on("error", reject);
  stream.write(input.subarray(0, 7));
  stream.end(input.subarray(7));
});
console.log("reset:", zlib.inflateSync(Buffer.concat(chunks), { dictionary }).equals(input));
stream.destroy();
