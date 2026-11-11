import { Readline } from "node:readline/promises";
import { Writable } from "node:stream";

const failure = new Error("write failed");
const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback(failure);
  },
});
output.on("error", () => {});
const rl = new Readline(output, { autoCommit: false });
const result = await rl.cursorTo(1).commit().catch((error) => error);
console.log(result === failure, result?.message);
output.destroy();
