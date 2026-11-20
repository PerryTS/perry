import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

for (const value of [undefined, 0, 50, 100.5, 5000, Infinity]) {
  const input = new PassThrough();
  const rl = createInterface({ input, terminal: false, crlfDelay: value });
  console.log(String(value), rl.crlfDelay);
  rl.close();
  input.destroy();
}
