import * as zlib from "node:zlib";

const compressed = zlib.deflateSync(
  "info contract",
  { info: true } as any,
) as any;
console.log("compress keys:", Object.keys(compressed).sort().join(","));
console.log("compress buffer:", Buffer.isBuffer(compressed.buffer));
console.log("compress engine:", compressed.engine instanceof zlib.Deflate);

const decompressed = zlib.inflateSync(
  compressed.buffer,
  { info: true } as any,
) as any;
console.log("decompress keys:", Object.keys(decompressed).sort().join(","));
console.log("decompress output:", decompressed.buffer.toString());
console.log("decompress engine:", decompressed.engine instanceof zlib.Inflate);
