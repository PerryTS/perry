import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const output = new PassThrough();
let written = "";
const onData = (chunk: Buffer) => written += chunk;
output.on("data", onData);
const rl = createInterface({
  input,
  output,
  terminal: true,
  completer(line, callback) {
    callback(null, [
      ["process.stdout", "process.stdin", "process.stderr"],
      line,
    ]);
  },
});
try {
  input.write("process.s\t");
  const first = written;
  input.write("\t");
  console.log(JSON.stringify([first, written.slice(first.length)]));
} finally {
  output.off("data", onData);
  rl.close();
  input.destroy();
  output.destroy();
}
