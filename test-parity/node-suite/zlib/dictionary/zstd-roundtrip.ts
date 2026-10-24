import * as zlib from "node:zlib";

const dictionary = Buffer.from(
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
);
const input = Buffer.from(
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
);
const compressed = zlib.zstdCompressSync(input, { dictionary } as any);
console.log(
  "with dictionary:",
  zlib.zstdDecompressSync(compressed, { dictionary } as any).equals(input),
);
try {
  zlib.zstdDecompressSync(compressed);
  console.log("without dictionary: ok");
} catch (error: any) {
  console.log("without dictionary:", error.name, error.code);
}

const padded = Buffer.concat([Buffer.from("prefix"), dictionary, Buffer.from("suffix")]);
const sliced = new DataView(padded.buffer, padded.byteOffset + 6, dictionary.length);
console.log("sliced view:", zlib.zstdDecompressSync(compressed, { dictionary: sliced }).equals(input));
try {
  zlib.zstdDecompressSync(compressed, { dictionary: Buffer.alloc(dictionary.length) });
  console.log("wrong dictionary: ok");
} catch (error: any) {
  console.log("wrong dictionary:", error.name, error.code);
}
const ordinary = zlib.zstdCompressSync(input);
console.log("ordinary:", ordinary.equals(zlib.zstdCompressSync(input, {})), zlib.zstdDecompressSync(ordinary).equals(input));

async function collect(stream: any, bytes: Buffer) {
  const chunks: Buffer[] = [];
  stream.on("data", (chunk: Buffer) => chunks.push(chunk));
  const done = new Promise<void>((resolve, reject) => {
    stream.on("end", resolve);
    stream.on("error", reject);
  });
  stream.write(bytes.subarray(0, 7));
  stream.end(bytes.subarray(7));
  await done;
  stream.destroy();
  return Buffer.concat(chunks);
}
const encoder = zlib.createZstdCompress({ dictionary: sliced });
encoder.reset();
const encoded = await collect(encoder, input);
console.log("reset encoder:", zlib.zstdDecompressSync(encoded, { dictionary }).equals(input));
try {
  zlib.zstdDecompressSync(encoded);
  console.log("stream without dictionary: ok");
} catch (error: any) {
  console.log("stream without dictionary:", error.name, error.code);
}
const decoder = zlib.createZstdDecompress({ dictionary: sliced });
decoder.reset();
console.log("reset decoder:", (await collect(decoder, compressed)).equals(input));

const compress = zlib.zstdCompressSync;
const decompress = zlib.zstdDecompressSync;
const view = new Uint8Array(padded.buffer, padded.byteOffset + 6, dictionary.length);
const captured = compress(input, { dictionary: view });
console.log("captured dictionary:", decompress(captured, { dictionary: view }).equals(input));
try {
  decompress(captured);
  console.log("captured without dictionary: ok");
} catch (error: any) {
  console.log("captured without dictionary:", error.name, error.code);
}
console.log("captured ordinary:", decompress(compress(input)).equals(input));
