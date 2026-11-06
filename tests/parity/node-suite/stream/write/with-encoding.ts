import { Writable } from "node:stream";
// write(chunk, encoding, cb) propagates the encoding to the underlying
// _write so chunks can be decoded correctly.
const seen: string[] = [];
const w = new Writable({
  write(chunk, enc, cb) { seen.push(`${enc}:${String(chunk)}`); cb(); },
});
w.on("finish", () => console.log("joined:", seen.join("|")));
w.write("hex-here", "hex");
w.end();
