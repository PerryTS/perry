import { Writable } from "node:stream";
// A Writable collects chunks via its `write(chunk, enc, cb)` and signals
// completion via the `finish` event after `end()`.
const chunks: string[] = [];
const w = new Writable({
  write(chunk, _enc, cb) {
    chunks.push(String(chunk));
    cb();
  },
});
w.on("finish", () => console.log("joined:", chunks.join(",")));
w.write("a");
w.write("b");
w.end("c");
