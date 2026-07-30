import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
if (typeof rl[Symbol.asyncIterator] === "function") {
  const iterator = rl[Symbol.asyncIterator]();
  input.end("one\n");
  for (let index = 0; index < 3; index++) {
    const result = await iterator.next();
    console.log(String(result.value), result.done);
  }
} else {
  console.log("missing");
  rl.close();
  input.destroy();
}
