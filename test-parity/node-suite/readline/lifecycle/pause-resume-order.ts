import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const events: string[] = [];
const rl = createInterface({ input, terminal: false });
for (const event of ["pause", "resume", "close"]) {
  rl.on(event, () => events.push(event));
}
rl.on("line", (line) => events.push(`line:${line}`));
console.log(rl.pause() === rl, rl.pause());
console.log(rl.resume() === rl, rl.resume());
input.end("x\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(events.join("|"));
rl.close();
input.destroy();
