import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const events: string[] = [];
const rl = createInterface({ input, terminal: false });
for (const event of ["pause", "resume", "close"]) {
  rl.on(event, () => events.push(event));
}
rl.on("line", (line) => events.push(`line:${line}`));
const firstPause = rl.pause();
const secondPause = rl.pause();
console.log(firstPause === rl, secondPause === rl);
const firstResume = rl.resume();
const secondResume = rl.resume();
console.log(firstResume === rl, secondResume === rl);
input.end("x\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(events.join("|"));
rl.close();
input.destroy();
