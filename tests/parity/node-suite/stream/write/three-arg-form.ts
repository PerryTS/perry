import { Writable } from "node:stream";
// write(chunk, encoding, callback) is the full 3-arg signature: cb runs
// after the write completes (or fails).
const received: string[] = [];
const w = new Writable({
  write(c, enc, cb) {
    received.push("enc=" + enc + " chunk=" + String(c));
    cb();
  },
});
let cbFired = false;
w.write("hello", "utf8", () => (cbFired = true));
w.end();
w.on("finish", () => {
  console.log("encoding seen:", received[0]);
  console.log("cb fired:", cbFired);
});
