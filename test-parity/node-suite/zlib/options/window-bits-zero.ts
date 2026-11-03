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

// Keep the zero exception separate from the normal nonzero bounds and types.
for (
  const [name, factory] of [
    ["inflate", zlib.createInflate],
    ["gunzip", zlib.createGunzip],
    ["unzip", zlib.createUnzip],
  ] as const
) {
  for (
    const value of [1, 2, 3, 4, 5, 6, 7, 8, 15, 16, undefined, NaN, null, "8", true]
  ) {
    try {
      const stream = factory({ windowBits: value } as any);
      console.log(name, "windowBits", String(value), "ok");
      stream.destroy();
    } catch (error: any) {
      console.log(
        name, "windowBits", String(value), error.name, error.code, error.message,
      );
    }
  }
}

// Observe the raw codec policy independently of the wrapped decoders.
for (
  const [name, factory] of [
    ["deflateRaw", zlib.createDeflateRaw],
    ["inflateRaw", zlib.createInflateRaw],
  ] as const
) {
  for (const value of [0, 8, 15, undefined]) {
    try {
      const stream = factory({ windowBits: value });
      console.log(name, "windowBits", String(value), "ok");
      stream.destroy();
    } catch (error: any) {
      console.log(
        name, "windowBits", String(value), error.name, error.code, error.message,
      );
    }
  }
}
