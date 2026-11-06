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

// The returned owner must account for the same operation that produced buffer.
import { deflateSync as namedDeflateSync, inflateSync as namedInflateSync } from "node:zlib";
const capturedDeflateSync = zlib.deflateSync;
const capturedInflateSync = zlib.inflateSync;
const plain = Buffer.from("info contract");
const expected = zlib.deflateSync(plain);
const routes = [
  ["named", namedDeflateSync, namedInflateSync],
  ["namespace", zlib.deflateSync, zlib.inflateSync],
  ["captured", capturedDeflateSync, capturedInflateSync],
] as const;
for (const [label, encode, decode] of routes) {
  const encoded = encode(plain, { info: true } as any) as any;
  const wrapped = Buffer.isBuffer(encoded.buffer);
  console.log(label, "encode", wrapped,
    wrapped && encoded.buffer.equals(expected),
    encoded.engine instanceof zlib.Deflate,
    encoded.engine?.bytesWritten,
    encoded.engine?._handle === null,
    encoded.engine?.destroyed, encoded.engine?.closed);
  const decoded = decode(expected, { info: true } as any) as any;
  console.log(label, "decode", Buffer.isBuffer(decoded.buffer),
    Buffer.isBuffer(decoded.buffer) && decoded.buffer.equals(plain),
    decoded.engine instanceof zlib.Inflate,
    decoded.engine?.bytesWritten === expected.length,
    decoded.engine?._handle === null,
    decoded.engine?.destroyed, decoded.engine?.closed);
  console.log(label, "default", Buffer.isBuffer(encode(plain)),
    Buffer.isBuffer(decode(expected)),
    encode(plain, { info: false } as any).equals(expected),
    decode(expected, { info: false } as any).equals(plain));
}
for (const info of [true, 1, "yes", false, 0, "", null, undefined]) {
  const output = zlib.deflateSync(plain, { info } as any) as any;
  console.log("truthy", String(info), Buffer.isBuffer(output),
    output.engine instanceof zlib.Deflate);
}
for (const bad of [false, true]) {
  const reads: string[] = [];
  const options = {
    get level() { reads.push("level"); return 6; },
    get chunkSize() { reads.push("chunkSize"); return 64; },
    get info() { reads.push("info"); return true; },
  };
  try {
    const output = zlib.deflateSync(bad ? 123 as any : plain, options) as any;
    console.log("getters", bad, reads.join(","), Buffer.isBuffer(output.buffer));
  } catch (error: any) {
    console.log("getters", bad, reads.join(","), error.name, error.code);
  }
}
const backing = Buffer.from("xinfo contracty");
const slice = backing.subarray(1, backing.length - 1);
const sliced = zlib.deflateSync(slice, { info: true } as any) as any;
console.log("slice", Buffer.isBuffer(sliced.buffer) && sliced.buffer.equals(expected),
  sliced.engine?.bytesWritten, backing.toString());
