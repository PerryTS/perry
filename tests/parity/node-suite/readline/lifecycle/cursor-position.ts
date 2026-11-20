import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
console.log(
  Object.keys(rl.getCursorPos()).join(","),
  JSON.stringify(rl.getCursorPos()),
);
rl.close();
input.destroy();
