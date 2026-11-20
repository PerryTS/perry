import { createInterface } from "node:readline/promises";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
let output = "";
const writable = new Writable({
  write(chunk, _encoding, callback) {
    output += chunk;
    callback();
  },
});
let seen = "";
const rl = createInterface({
  input,
  output: writable,
  terminal: true,
  async completer(line) {
    seen = line;
    return [["Input"], line] as [string[], string];
  },
});
input.write("input\t");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(seen, output.includes("> Input"));
rl.close();
input.destroy();
writable.destroy();
