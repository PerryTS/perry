import * as readline from "node:readline";
import { Writable } from "node:stream";

const writes: string[] = [];
const callbacks: string[] = [];

const output = new Writable({
  write(chunk, _encoding, callback) {
    writes.push(String(chunk));
    callback();
  },
});

console.log("clearLine ret:", readline.clearLine(output, 0, () => callbacks.push("clearLine")));
console.log("clearScreenDown ret:", readline.clearScreenDown(output, () => callbacks.push("clearScreenDown")));
console.log("cursorTo ret:", readline.cursorTo(output, 2, 3, () => callbacks.push("cursorTo")));
console.log("moveCursor ret:", readline.moveCursor(output, -1, 2, () => callbacks.push("moveCursor")));

await new Promise<void>((resolve) => setImmediate(resolve));

console.log("writes:", JSON.stringify(writes.join("")));
console.log("callbacks:", callbacks.join("|"));
