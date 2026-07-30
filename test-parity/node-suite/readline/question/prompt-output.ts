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
const rl = createInterface({ input, output: writable, terminal: false });
rl.question("ask> ", () => {});
console.log(JSON.stringify(output));
rl.close();
input.destroy();
writable.destroy();
