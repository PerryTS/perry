import * as readline from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const writes: string[] = [];
const answers: string[] = [];

const output = new Writable({
  write(chunk, _encoding, callback) {
    writes.push(String(chunk));
    callback();
  },
});

const rl = readline.createInterface({ input, output, terminal: false });
rl.question("ask> ", (answer) => {
  answers.push(answer);
});

input.end("answer\n");
await new Promise<void>((resolve) => setImmediate(resolve));
rl.close();

console.log("answers:", answers.join("|"));
console.log("writes:", JSON.stringify(writes.join("")));
