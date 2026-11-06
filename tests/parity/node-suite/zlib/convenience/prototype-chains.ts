import * as zlib from "node:zlib";

for (
  const [name, parent] of [
    ["Deflate", "Zlib"],
    ["DeflateRaw", "Zlib"],
    ["Gzip", "Zlib"],
    ["Gunzip", "Zlib"],
    ["Inflate", "Zlib"],
    ["InflateRaw", "Zlib"],
    ["Unzip", "Zlib"],
    ["BrotliCompress", "Brotli"],
    ["BrotliDecompress", "Brotli"],
  ] as const
) {
  const Ctor = (zlib as any)[name];
  const descriptor = Object.getOwnPropertyDescriptor(
    Ctor.prototype,
    "constructor",
  )!;
  console.log(
    name,
    Ctor.prototype.constructor === Ctor,
    Object.getPrototypeOf(Ctor.prototype).constructor.name === parent,
    descriptor.enumerable,
    descriptor.writable,
    descriptor.configurable,
  );
}
