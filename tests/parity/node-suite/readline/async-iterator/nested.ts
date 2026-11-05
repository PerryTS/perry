import { createInterface } from "node:readline";
import { Readable } from "node:stream";

const input = Readable.from(["first\nsecond\nthird\n"]);
const rl = createInterface({ input, terminal: false });
if (typeof rl[Symbol.asyncIterator] === "function") {
  const lines: string[] = [];
  let outerCount = 0;
  try {
    for await (const outer of rl) {
      outerCount++;
      lines.push(outer);
      for await (const inner of rl) lines.push(inner);
    }
    console.log(outerCount, JSON.stringify(lines));
  } finally {
    rl.close();
    input.destroy();
  }
} else {
  console.log("missing");
  rl.close();
  input.destroy();
}
