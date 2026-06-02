import { Writable } from "node:stream";
// User-provided writev(chunks, cb) receives chunks as an array of
// { chunk, encoding } objects.
let chunkShape: any = null;
const w = new Writable({
  writev(chunks: any[], cb: any) {
    if (chunks.length > 0 && !chunkShape) {
      const first = chunks[0];
      chunkShape = {
        isArray: Array.isArray(chunks),
        hasChunk: "chunk" in first,
        hasEncoding: "encoding" in first,
        chunkType: typeof first.chunk,
      };
    }
    cb();
  },
});
w.cork();
w.write("a");
w.write("b");
w.uncork();
w.end();
w.on("finish", () => console.log("shape:", JSON.stringify(chunkShape)));
