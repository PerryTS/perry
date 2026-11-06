import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({
  input,
  terminal: false,
  signal: AbortSignal.abort(),
});
let closes = 0;
rl.on("close", () => closes++);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(closes, rl.closed);
rl.close();
input.destroy();
