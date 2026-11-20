import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const lines: string[] = [];
const rl = createInterface({ input, terminal: false });
rl.on("line", (line) => lines.push(line));
for (const byte of Buffer.from("☮")) input.write(Buffer.from([byte]));
input.end("\n");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(JSON.stringify(lines));
rl.close();
input.destroy();
