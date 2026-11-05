// #11620: zlib transform streams are async-iterable.
import * as zlib from "node:zlib";
import * as stream from "node:stream";
const gz = zlib.gzipSync(new Uint8Array(200000).fill(65));
async function* src() { yield gz.subarray(0, 100); yield gz.subarray(100); }
async function drain(label: string, s: AsyncIterable<Uint8Array>) {
  let n = 0;
  for await (const c of s) n += c.length;
  console.log(label, n);
}
await drain("piped", stream.Readable.from(src()).pipe(zlib.createGunzip()));
const direct = zlib.createGunzip();
direct.end(gz);
await drain("direct", direct);
const later = zlib.createGunzip();
setTimeout(() => { later.write(gz.subarray(0, 50)); later.end(gz.subarray(50)); }, 10);
await drain("later writes", later);
const early = zlib.createGunzip();
early.end(gz);
for await (const c of early) { console.log("break after first:", c.length > 0); break; }
console.log("done");
