import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: true });
const snapshots: string[] = [];
rl.on("history", (history) => snapshots.push(history.join(",")));
input.write("one\ntwo\nthree\n");
console.log(snapshots.join("|"));
rl.close();
input.destroy();
output.destroy();
