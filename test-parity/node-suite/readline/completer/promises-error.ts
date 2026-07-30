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
const rl = createInterface({
  input,
  output: writable,
  terminal: true,
  completer: async () => {
    throw new Error("message");
  },
});
input.write("\t");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(output.startsWith("Tab completion error: Error: message"));
rl.close();
input.destroy();
writable.destroy();
