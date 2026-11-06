import { createInterface } from "node:readline/promises";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = createInterface({ input, output, terminal: false });
const result = await rl.question("q> ", { signal: AbortSignal.abort("reason") })
  .catch((error) => error);
console.log(result.name, result.code, result.cause);
rl.close();
input.destroy();
output.destroy();
