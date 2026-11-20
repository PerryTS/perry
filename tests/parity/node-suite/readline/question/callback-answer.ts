import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: false });
let answer = "missing";
rl.question("ask> ", (value) => answer = value);
input.end("answer\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(answer);
rl.close();
input.destroy();
output.destroy();
