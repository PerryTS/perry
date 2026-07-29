import * as zlib from "node:zlib";

for (
  const [name, factory] of [
    ["gzip", zlib.createGzip],
    ["deflate", zlib.createDeflate],
    ["inflate", zlib.createInflate],
    ["gunzip", zlib.createGunzip],
    ["unzip", zlib.createUnzip],
  ] as const
) {
  try {
    const stream = factory({ windowBits: 0 });
    console.log(name, "ok");
    stream.destroy();
  } catch (error: any) {
    console.log(name, error.name, error.code);
  }
}
