import { cursorTo } from "node:readline";
import { Writable } from "node:stream";

const output = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
for (
  const call of [
    () => cursorTo(output, NaN),
    () => cursorTo(output, undefined as any, 1),
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
