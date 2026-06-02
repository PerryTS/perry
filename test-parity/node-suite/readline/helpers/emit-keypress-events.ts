import * as readline from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const events: string[] = [];

readline.emitKeypressEvents(input);
input.on("keypress", (str, key) => {
  const printable = str === undefined ? "undefined" : JSON.stringify(str);
  events.push(`${printable}:${key.name}:${key.ctrl}:${key.shift}:${JSON.stringify(key.sequence)}`);
});

input.write("a");
input.write("\u001b[A");

await new Promise<void>((resolve) => setImmediate(resolve));

console.log("events:", events.join("|"));
