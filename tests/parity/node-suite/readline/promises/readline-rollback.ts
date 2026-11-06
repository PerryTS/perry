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
console.log(rl.cursorTo(9, 1).rollback() === rl);
await rl.commit();
console.log(JSON.stringify(output));
writable.destroy();
