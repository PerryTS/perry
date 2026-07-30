import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
try {
  const rl = createInterface(input, output, undefined, false);
  console.log(rl.input === input, rl.output === output, rl.terminal);
  rl.close();
} catch (error: any) {
  console.log(error.name, error.message);
}
input.destroy();
output.destroy();
