import * as zlib from "node:zlib";

const gzip = zlib.createGzip();

console.log("params typeof:", typeof gzip.params);
console.log("reset typeof:", typeof gzip.reset);
console.log("bytesWritten typeof:", typeof gzip.bytesWritten);
console.log("bytesWritten initial:", gzip.bytesWritten);
console.log("bytesRead typeof:", typeof (gzip as any).bytesRead);

gzip.destroy();

const written = zlib.createGzip();
written.on("data", () => {});
const finished = new Promise<void>((resolve, reject) => {
  written.on("end", () => {
    console.log("bytesWritten on end:", written.bytesWritten);
    resolve();
  });
  written.on("error", reject);
});

written.end(Buffer.from("abc"));
console.log("bytesWritten after end call:", written.bytesWritten);

await finished;
