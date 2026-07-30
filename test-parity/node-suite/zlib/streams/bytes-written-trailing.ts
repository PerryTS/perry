import * as zlib from "node:zlib";

const compressed = zlib.deflateSync("bytes written");
const input = Buffer.concat([compressed, Buffer.from("trailing")]);
const stream = zlib.createInflate();
const chunks: Buffer[] = [];
stream.on("data", (chunk: Buffer) => chunks.push(chunk));
await new Promise<void>((resolve, reject) => {
  stream.on("end", resolve);
  stream.on("error", reject);
  stream.end(input);
});
console.log("output:", Buffer.concat(chunks).toString());
console.log(
  "bytesWritten:",
  stream.bytesWritten,
  compressed.length,
  input.length,
);
stream.destroy();
