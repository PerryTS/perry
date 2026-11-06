import { Writable } from "node:stream";
// write(Buffer) passes the Buffer through to the underlying _write handler.
const seen: number[] = [];
const w = new Writable({
  write(chunk, _e, cb) { seen.push(chunk.length); cb(); },
});
w.on("finish", () => console.log("lengths:", seen.join(",")));
w.write(Buffer.from("ab"));
w.write(Buffer.from("cde"));
w.end();
