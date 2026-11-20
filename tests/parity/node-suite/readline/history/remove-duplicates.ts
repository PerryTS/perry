import { createInterface } from "node:readline";
import { PassThrough, Writable } from "node:stream";

for (const removeHistoryDuplicates of [false, true]) {
  const input = new PassThrough();
  const output = new Writable({
    write(_chunk, _encoding, callback) {
      callback();
    },
  });
  const rl = createInterface({
    input,
    output,
    terminal: true,
    removeHistoryDuplicates,
  });
  input.write("one\ntwo\none\n");
  console.log(removeHistoryDuplicates, JSON.stringify(rl.history));
  rl.close();
  input.destroy();
  output.destroy();
}
