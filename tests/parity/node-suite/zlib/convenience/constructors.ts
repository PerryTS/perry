import * as zlib from "node:zlib";

for (
  const name of [
    "Deflate",
    "DeflateRaw",
    "Gzip",
    "Gunzip",
    "Inflate",
    "InflateRaw",
    "Unzip",
    "BrotliCompress",
    "BrotliDecompress",
  ] as const
) {
  const Ctor = zlib[name] as any;
  const called = Ctor();
  const constructed = new Ctor();
  console.log(
    name,
    called instanceof Ctor,
    constructed instanceof Ctor,
    Ctor.name,
  );
  called.destroy();
  constructed.destroy();
}
