import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });

console.log(typeof rl[Symbol.asyncIterator], typeof rl[Symbol.dispose]);
rl.close();
input.destroy();
