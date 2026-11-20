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
const completer = (
  line: string,
  callback: (error: Error | null, result: [string[], string]) => void,
) => {
  seen = line;
  queueMicrotask(() => callback(null, [["Input"], line]));
};
const rl = createInterface({
  input,
  output: writable,
  terminal: true,
  completer,
});
input.write("input\t");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(seen, output.includes("> Input"));
rl.close();
input.destroy();
writable.destroy();
