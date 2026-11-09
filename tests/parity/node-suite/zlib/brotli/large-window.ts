// RFC 7932 caps the Brotli window at 16 MiB. A large-window stream (window up
// to 1 GiB) decodes only when BROTLI_DECODER_PARAM_LARGE_WINDOW is set; every
// decoder entry point rejects it otherwise.
import {
  brotliDecompress,
  brotliDecompressSync,
  constants,
  createBrotliDecompress,
} from "node:zlib";

// Node: brotliCompressSync(text, { params: { LARGE_WINDOW: 1, LGWIN: 30, QUALITY: 5 } })
const encoded = Buffer.from(
  "EV60AAAQt7F1Had7bAmRYIFDTg/qXyBhglRbdICkEE+lDjD+EXRY/02MalvSwAA=",
  "base64",
);
const largeWindow = { params: { [constants.BROTLI_DECODER_PARAM_LARGE_WINDOW]: 1 } };

function describe(err: any): string {
  return `${err.name} ${err.code} ${err.message}`;
}

try {
  brotliDecompressSync(encoded);
  console.log("sync default: decoded");
} catch (err) {
  console.log("sync default:", describe(err));
}
console.log("sync large:", brotliDecompressSync(encoded, largeWindow).toString());

function streamed(label: string, options: object | undefined, next: () => void) {
  const chunks: Buffer[] = [];
  const decoder = createBrotliDecompress(options);
  decoder.on("data", (chunk: Buffer) => chunks.push(chunk));
  decoder.on("error", (err) => {
    console.log(label, describe(err));
    next();
  });
  decoder.on("end", () => {
    console.log(label, Buffer.concat(chunks).toString());
    next();
  });
  decoder.end(encoded);
}

brotliDecompress(encoded, (err, out) => {
  console.log("callback default:", err ? describe(err) : out.toString());
  brotliDecompress(encoded, largeWindow, (err, out) => {
    console.log("callback large:", err ? describe(err) : out.toString());
    streamed("stream default:", undefined, () => {
      streamed("stream large:", largeWindow, () => console.log("done"));
    });
  });
});
