import { cursorTo } from "node:readline";
import { Writable } from "node:stream";

let output = "";
const writable = new Writable({
  write(chunk, _encoding, callback) {
    output += chunk;
    callback();
  },
});
console.log(cursorTo(writable, 1), JSON.stringify(output));
output = "";
console.log(cursorTo(writable, 1, 2), JSON.stringify(output));
output = "";
const callbacks: string[] = [];
cursorTo(writable, 2, (error) => callbacks.push(String(error)));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(callbacks.join("|"), JSON.stringify(output));
writable.destroy();
