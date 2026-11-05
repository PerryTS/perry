import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

const input = new PassThrough();
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
(output as any).isTTY = true;
const inferred = createInterface({ input, output });
const overridden = createInterface({ input, output, terminal: false });

console.log(inferred.terminal, overridden.terminal);
inferred.close();
overridden.close();
input.destroy();
output.destroy();
