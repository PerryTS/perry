import { Readline } from "node:readline/promises";
import { Writable } from "node:stream";

let output = "";
const writable = new Writable({
  write(chunk, _encoding, callback) {
    output += chunk;
    callback();
  },
});
const rl = new Readline(writable, { autoCommit: true });
console.log(rl.clearLine(1) === rl, JSON.stringify(output));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(JSON.stringify(output));
writable.destroy();
