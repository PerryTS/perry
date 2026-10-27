// zlib through the bundled stdlib: one-shot codecs, a streaming pipeline and
// checksums, in memory.
import * as zlib from "node:zlib";

const text = Buffer.from("perry ".repeat(500));
const gz = zlib.gzipSync(text);
console.log(gz.length < text.length, zlib.gunzipSync(gz).equals(text));
console.log(zlib.inflateSync(zlib.deflateSync(text, { level: 9 })).length);
console.log(zlib.inflateRawSync(zlib.deflateRawSync(text)).toString().slice(0, 12));
console.log(zlib.crc32("hello"));

const deflate = zlib.createDeflate();
const parts: Buffer[] = [];
deflate.on("data", (c: Buffer) => parts.push(c));
deflate.on("end", () => {
  console.log("stream", zlib.inflateSync(Buffer.concat(parts)).toString());
});
deflate.write("stream ");
deflate.end("done");
