// #10810 — building Perry's CLI must not select a lower-ratio flate2 backend
// for node:zlib through Cargo feature unification. The former backend produced
// roughly 4.2 KiB here; Node and the intended backend stay below 3 KiB.
import { gzip, gzipSync } from "node:zlib";
import { promisify } from "node:util";

const payload = Buffer.alloc(512 * 1024);
for (let i = 0; i < payload.length; i++) payload[i] = i % 251;

function gzipCallback(data: Buffer): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    gzip(data, (error: Error | null, output: Buffer) =>
      error ? reject(error) : resolve(output)
    );
  });
}

const isCompact = (output: Buffer): boolean => output.length < 3_000;

console.log("gzipSync default is compact:", isCompact(gzipSync(payload)));
console.log(
  "gzip callback default is compact:",
  isCompact(await gzipCallback(payload))
);
const gzipPromise = promisify(gzip);
console.log(
  "promisified gzip default is compact:",
  isCompact((await gzipPromise(payload)) as Buffer)
);
