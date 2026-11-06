import * as zlib from "node:zlib";

async function collect(reset: (stream: any, callback: () => void) => void) {
  const deflate = zlib.createDeflate();
  const inflate = zlib.createInflate();
  const chunks: Buffer[] = [];
  inflate.on("data", (chunk: Buffer) => chunks.push(chunk));
  const settled = new Promise<void>((resolve, reject) => {
    inflate.on("end", resolve);
    inflate.on("error", reject);
    deflate.on("error", reject);
  });
  deflate.pipe(inflate);
  reset(deflate, () => deflate.end("abc"));
  await settled;
  deflate.destroy();
  inflate.destroy();
  return Buffer.concat(chunks).toString();
}

console.log(
  "reset:",
  await collect((stream, callback) => {
    stream.reset();
    callback();
  }),
);
console.log(
  "params:",
  await collect((stream, callback) => {
    stream.params(0, zlib.constants.Z_DEFAULT_STRATEGY, callback);
  }),
);
