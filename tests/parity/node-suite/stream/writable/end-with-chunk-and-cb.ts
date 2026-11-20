import { Writable } from "node:stream";
// end(chunk, encoding, callback) writes a final chunk + invokes cb on finish.
const seen: string[] = [];
const w = new Writable({
  write(c, _e, cb) { seen.push(String(c)); cb(); },
});
w.end("final", () => console.log("cb fired, joined:", seen.join("")));
