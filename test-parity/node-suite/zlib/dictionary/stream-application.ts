import * as zlib from "node:zlib";

const dictionary = Buffer.from("stream dictionary words");
const input = Buffer.from("stream dictionary words stream dictionary words");
const stream = zlib.createDeflate({ dictionary });
const chunks: Buffer[] = [];
stream.on("data", (chunk: Buffer) => chunks.push(chunk));
await new Promise<void>((resolve, reject) => {
  stream.on("end", resolve);
  stream.on("error", reject);
  stream.end(input);
});
const compressed = Buffer.concat(chunks);
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
stream.destroy();
