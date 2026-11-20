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
const answers: string[] = [];
rl.question("first? ", (answer) => answers.push(`first:${answer}`));
rl.question("second? ", (answer) => answers.push(`second:${answer}`));
input.write("value\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(JSON.stringify(output), answers.join("|"));
rl.close();
input.destroy();
writable.destroy();
