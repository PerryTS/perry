import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
let closes = 0;
rl.on("close", () => closes++);
if (typeof rl[Symbol.asyncIterator] === "function") {
  input.end("one\ntwo\n");
  for await (const line of rl) {
    console.log(line);
    break;
  }
  console.log(closes, rl.closed);
} else {
  console.log("missing");
  rl.close();
}
input.destroy();
