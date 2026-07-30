import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: true, historySize: 2 });
input.write("one\ntwo\nthree\n");
console.log(JSON.stringify(rl.history));
rl.close();
input.destroy();
output.destroy();
