import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const events: string[] = [];
const rl = createInterface({ input, terminal: false });
rl.on("resume", () => events.push("resume"));
rl.on("line", (line) => events.push(`line:${line}`));
rl.pause();
rl.write("answer\n");
console.log(rl.paused, events.join("|"));
rl.close();
input.destroy();
