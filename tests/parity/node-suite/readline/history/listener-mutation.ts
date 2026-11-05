import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: true });
let line = "";
rl.on("history", (history) => history.shift());
rl.on("line", (value) => line = value);
input.write("value\n");
console.log(line, JSON.stringify(rl.history));
rl.close();
input.destroy();
output.destroy();
