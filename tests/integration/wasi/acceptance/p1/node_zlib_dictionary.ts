// Deflate that switches level and strategy mid-stream with params() round-trips
// through inflate: zlib-wrapped with a dictionary, and raw without one (the raw
// codecs do not take a dictionary yet). A missing or wrong dictionary reports
// the codec's own error codes.
import * as zlib from "node:zlib";

const dictionary = Buffer.from("perry runtime dictionary ".repeat(40));
const first = Buffer.from("perry runtime dictionary: first chunk, ".repeat(30));
const second = Buffer.from("tail with other words entirely, ".repeat(30));
const third = Buffer.from("perry runtime dictionary ".repeat(30));

function encode(kind: "Deflate" | "DeflateRaw", done: (out: Buffer) => void): void {
  const opts = kind === "Deflate" ? { dictionary, level: 6 } : { level: 6 };
  const codec: any = (zlib as any)["create" + kind](opts);
  const chunks: Buffer[] = [];
  codec.on("data", (c: Buffer) => chunks.push(c));
  codec.on("end", () => done(Buffer.concat(chunks)));
  codec.write(first);
  codec.params(0, zlib.constants.Z_DEFAULT_STRATEGY, () => {
    codec.write(second);
    codec.params(9, zlib.constants.Z_FILTERED, () => {
      codec.end(third);
    });
  });
}

const expected = Buffer.concat([first, second, third]);
function code(fn: () => void): string {
  try {
    fn();
    return "no error";
  } catch (e: any) {
    return `${e.code} ${e.message}`;
  }
}

encode("Deflate", (wrapped) => {
  console.log("zlib", zlib.inflateSync(wrapped, { dictionary }).equals(expected));
  console.log("missing", code(() => zlib.inflateSync(wrapped)));
  console.log("bad", code(() => zlib.inflateSync(wrapped, { dictionary: Buffer.from("not the dictionary") })));
  encode("DeflateRaw", (raw) => {
    console.log("raw", zlib.inflateRawSync(raw).equals(expected));
  });
});
