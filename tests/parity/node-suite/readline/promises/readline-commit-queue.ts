import { Readline } from "node:readline/promises";
import { Writable } from "node:stream";

let output = "";
const writable = new Writable({
  write(chunk, _encoding, callback) {
    output += chunk;
    callback();
  },
});
const rl = new Readline(writable, { autoCommit: false });
const chain = rl.clearLine(0).cursorTo(3, 4).moveCursor(2, -1)
  .clearScreenDown();
console.log(chain === rl, JSON.stringify(output));
console.log(await rl.commit(), JSON.stringify(output));
writable.destroy();
