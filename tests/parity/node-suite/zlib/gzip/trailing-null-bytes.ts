import * as zlib from "node:zlib";

const input = Buffer.concat([
  zlib.gzipSync("abc"),
  zlib.gzipSync("def"),
  Buffer.alloc(4),
]);

console.log("sync:", zlib.gunzipSync(input).toString());
await new Promise<void>((resolve) => {
  zlib.gunzip(input, (error, output) => {
    console.log("async:", error === null, output.toString());
    resolve();
  });
});
