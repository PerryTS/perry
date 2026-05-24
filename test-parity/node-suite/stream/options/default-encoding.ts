import { Writable } from "node:stream";
// Writable's defaultEncoding option controls the encoding used when
// write(chunk) is called without an explicit encoding arg.
let seenEnc: any = null;
const w = new Writable({
  defaultEncoding: "hex",
  write(_c, enc, cb) {
    if (!seenEnc) seenEnc = enc;
    cb();
  },
});
w.write("abcdef");
w.end();
w.on("finish", () => console.log("default encoding:", seenEnc));
