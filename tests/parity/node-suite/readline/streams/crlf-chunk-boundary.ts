import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const lines: string[] = [];
const rl = createInterface({ input, terminal: false, crlfDelay: Infinity });
rl.on("line", (line) => lines.push(line));
input.write("foo\r");
input.write("\nbar\r");
input.end("\nbaz");
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(JSON.stringify(lines));
rl.close();
input.destroy();
