import zlib from "node:zlib";

// stream.params(level, strategy, cb) retunes the compression level for
// subsequent writes. Encoding the same repeated input at level 0 (store) vs
// level 9 (max) must yield different valid deflate output that still round-trips.
function encode(level: number, done: (out: Buffer) => void): void {
  const chunks: Buffer[] = [];
  const stream = zlib.createDeflate({ level: 6 });
  stream.on("data", (c: Buffer) => chunks.push(c));
  stream.on("end", () => done(Buffer.concat(chunks)));
  stream.params(level, zlib.constants.Z_DEFAULT_STRATEGY, (err: Error | null) => {
    if (err) throw err;
    stream.end(Buffer.from("a".repeat(32768)));
  });
}

encode(0, (level0) => {
  encode(9, (level9) => {
    // Level 0 (store) is much larger than level 9 (max compression) here.
    console.log("level0Bigger:", level0.length > level9.length);
    // Both must decode back to the original 32768 bytes.
    console.log("rt0:", zlib.inflateSync(level0).length);
    console.log("rt9:", zlib.inflateSync(level9).length);
  });
});

// Validation: invalid level / strategy / type throw synchronously.
function caught(fn: () => void): string {
  try {
    fn();
    return "no-throw";
  } catch (e: any) {
    return e.code || e.name;
  }
}
console.log(
  "badLevel:",
  caught(() => zlib.createDeflate().params(99, zlib.constants.Z_DEFAULT_STRATEGY, () => {})),
);
console.log(
  "badStrategy:",
  caught(() => zlib.createDeflate().params(1, 999, () => {})),
);
console.log(
  "badType:",
  caught(() => zlib.createDeflate().params("1" as any, zlib.constants.Z_DEFAULT_STRATEGY, () => {})),
);
