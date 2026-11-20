import * as zlib from "node:zlib";

const input = Buffer.concat([zlib.gzipSync("abc"), zlib.gzipSync("def")]);
const stream = zlib.createUnzip();
const chunks: Buffer[] = [];
stream.on("data", (chunk: Buffer) => chunks.push(chunk));
await new Promise<void>((resolve, reject) => {
  stream.on("end", resolve);
  stream.on("error", reject);
  for (const byte of input) stream.write(Buffer.from([byte]));
  stream.end();
});
console.log("output:", Buffer.concat(chunks).toString());
console.log("bytesWritten:", stream.bytesWritten, input.length);
stream.destroy();
