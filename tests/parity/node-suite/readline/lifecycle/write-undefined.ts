import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
let lines = 0;
rl.on("line", () => lines++);
console.log(rl.write(), lines, JSON.stringify(rl.line));
rl.close();
input.destroy();
