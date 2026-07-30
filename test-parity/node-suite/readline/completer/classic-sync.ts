import { createInterface } from "node:readline";
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
  completer(line) {
    seen = line;
    return [["Input"], line];
  },
});
input.write("input\t");
await new Promise<void>((resolve) => queueMicrotask(resolve));
console.log(seen, output.includes("> Input"));
rl.close();
input.destroy();
writable.destroy();
