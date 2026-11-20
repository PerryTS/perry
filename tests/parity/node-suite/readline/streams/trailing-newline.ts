import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const lines: string[] = [];
const rl = createInterface({ input, terminal: false });
rl.on("line", (line) => lines.push(line));
input.end("one\ntwo\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(JSON.stringify(lines));
rl.close();
input.destroy();
