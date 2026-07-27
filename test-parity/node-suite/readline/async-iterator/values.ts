import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
if (typeof rl[Symbol.asyncIterator] === "function") {
  input.end("\nalpha\nlast");
  const lines: string[] = [];
  for await (const line of rl) lines.push(line);
  console.log(JSON.stringify(lines));
} else {
  console.log("missing");
  rl.close();
  input.destroy();
}
