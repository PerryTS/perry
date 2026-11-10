import { createGzip, createGunzip, createDeflate, createInflate,
         createDeflateRaw, createInflateRaw, createUnzip,
         createBrotliCompress, createBrotliDecompress } from "node:zlib";

// Constructing each factory must yield an object that exposes the Transform
// core: write/end/on/pipe. We avoid binding lifecycles here — just probe the
// shape so this stays deterministic.
for (const [name, stream] of [
  ["gzip", createGzip()],
  ["gunzip", createGunzip()],
  ["deflate", createDeflate()],
  ["inflate", createInflate()],
  ["deflateRaw", createDeflateRaw()],
  ["inflateRaw", createInflateRaw()],
  ["unzip", createUnzip()],
  ["brotliCompress", createBrotliCompress()],
  ["brotliDecompress", createBrotliDecompress()],
] as const) {
  console.log(name + " write:", typeof (stream as any).write);
  console.log(name + " end:", typeof (stream as any).end);
  console.log(name + " on:", typeof (stream as any).on);
  console.log(name + " pipe:", typeof (stream as any).pipe);
}
