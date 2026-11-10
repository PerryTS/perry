import { Writable } from "node:stream";
// write('') is a no-op write that still triggers the callback chain;
// underlying _write receives an empty Buffer (decodeStrings:true default).
let sawEmpty = false;
const w = new Writable({
  write(chunk, _e, cb) { if (chunk.length === 0) sawEmpty = true; cb(); },
});
w.on("finish", () => console.log("empty write seen:", sawEmpty));
w.write("");
w.end();
