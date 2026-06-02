import { createInterface } from "node:readline/promises";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const writes: string[] = [];
const output = new Writable({
  write(chunk, _encoding, callback) {
    writes.push(String(chunk));
    callback();
  },
});

const rl = createInterface({ input, output, terminal: false });
const answerPromise = rl.question("ask> ");

input.end("answer\r\n");
const answer = await answerPromise;
rl.close();

console.log("answer:", answer);
console.log("writes:", JSON.stringify(writes.join("")));
console.log("surface:", typeof rl.question, typeof rl.close);
