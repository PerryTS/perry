import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });

console.log(rl.getPrompt(), rl.tabSize, rl.escapeCodeTimeout, rl.historySize);
console.log(rl.terminal, rl.line, rl.cursor);
rl.close();
input.destroy();
