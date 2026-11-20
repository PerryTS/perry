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
const completer = (_line: string, callback: (error: Error) => void) =>
  callback(new Error("message"));
const rl = createInterface({
  input,
  output: writable,
  terminal: true,
  completer,
});
input.write("\t");
await new Promise<void>((resolve) => queueMicrotask(resolve));
console.log(output.startsWith("Tab completion error: Error: message"));
rl.close();
input.destroy();
writable.destroy();
