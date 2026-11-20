import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
let closes = 0;
rl.on("close", () => closes++);
console.log(rl.close(), rl.close(), closes, rl.closed);
input.destroy();
