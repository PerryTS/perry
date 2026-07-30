import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const output = new PassThrough();
const rl = createInterface({ input, output, terminal: true });
let depth = 0;
const onLine = () => {
  depth++;
  if (depth <= 2) rl.write("foo");
};
rl.on("line", onLine);
try {
  rl.write(" \n}\n");
  console.log(depth);
} finally {
  rl.off("line", onLine);
  rl.close();
  input.destroy();
  output.destroy();
}
