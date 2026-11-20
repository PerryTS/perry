import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: false });
let receiver: unknown;
let answer = "missing";
rl.question("q> ", function (value) {
  receiver = this;
  answer = value;
});
input.end("yes\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(answer, receiver === undefined);
rl.close();
input.destroy();
output.destroy();
