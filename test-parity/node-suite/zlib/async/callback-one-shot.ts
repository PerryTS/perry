import * as zlib from "node:zlib";

const input = Buffer.from("perry zlib callback coverage");

type Callback = (err: any, out: Buffer) => void;

function errName(err: any): string {
  return err === null ? "null" : err === undefined ? "undefined" : err.name;
}

async function run(
  label: string,
  invoke: (callback: Callback) => any,
  describe: (out: Buffer) => string | boolean,
) {
  let line = "";
  let done!: () => void;
  const settled = new Promise<void>((resolve) => {
    done = resolve;
  });
  const ret = invoke((err, out) => {
    line = `${label} callback: ${errName(err)} ${Buffer.isBuffer(out)} ${describe(out)}`;
    done();
  });
  console.log(`${label} return:`, ret === undefined ? "undefined" : typeof ret);
  await settled;
  console.log(line);
}

await run("gzip", (cb) => zlib.gzip(input, cb), (out) => out.length > 0);
await run("gunzip", (cb) => zlib.gunzip(zlib.gzipSync(input), cb), (out) => out.toString());
await run("deflate", (cb) => zlib.deflate(input, cb), (out) => out.length > 0);
await run("inflate", (cb) => zlib.inflate(zlib.deflateSync(input), cb), (out) => out.toString());
await run("deflateRaw", (cb) => zlib.deflateRaw(input, cb), (out) => out.length > 0);
await run(
  "inflateRaw",
  (cb) => zlib.inflateRaw(zlib.deflateRawSync(input), cb),
  (out) => out.toString(),
);
await run("unzip", (cb) => zlib.unzip(zlib.gzipSync(input), cb), (out) => out.toString());
await run("brotliCompress", (cb) => zlib.brotliCompress(input, cb), (out) => out.length > 0);
await run(
  "brotliDecompress",
  (cb) => zlib.brotliDecompress(zlib.brotliCompressSync(input), cb),
  (out) => out.toString(),
);
