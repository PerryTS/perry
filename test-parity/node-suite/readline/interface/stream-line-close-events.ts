import * as readline from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const events: string[] = [];

const rl = readline.createInterface({ input, output, terminal: false });
rl.on("line", (line) => {
  events.push(`line:${line}`);
});
rl.on("close", () => {
  events.push("close");
});

input.write("alpha\nbeta\r\n");
input.end();

await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));

console.log("events:", events.join("|"));
