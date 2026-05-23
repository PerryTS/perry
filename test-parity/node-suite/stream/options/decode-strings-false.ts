import { Writable } from "node:stream";
// decodeStrings: false keeps string writes as strings (instead of decoding
// to Buffer) when they reach _write.
const seen: string[] = [];
const w = new Writable({
  decodeStrings: false,
  write(chunk, _e, cb) { seen.push(typeof chunk); cb(); },
});
w.on("finish", () => console.log("types:", seen.join(",")));
w.write("plain");
w.end();
