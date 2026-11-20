import { Readline } from "node:readline/promises";
import { Writable } from "node:stream";

const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
const rl = new Readline(output, { autoCommit: false });
for (
  const call of [
    () => rl.cursorTo("1" as any),
    () => rl.cursorTo(1.5),
    () => rl.moveCursor(NaN, 0),
    () => rl.clearLine("0" as any),
  ]
) {
  try {
    call();
    console.log("ok");
  } catch (error: any) {
    console.log(error.name, error.code);
  }
}
output.destroy();
