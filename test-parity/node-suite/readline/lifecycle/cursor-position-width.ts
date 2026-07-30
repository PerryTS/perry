import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const output = new PassThrough();
const rl = createInterface({ input, output, terminal: true, prompt: "" });
const cases = [
  "a",
  "ab",
  "丁",
  "\u0301",
  "a\u0301",
  "\u20dd",
  "a\u20ddb",
  "\u200e",
];
try {
  const columns: number[] = [];
  for (const value of cases) {
    rl.write(value);
    columns.push(rl.getCursorPos().cols);
    rl.write(null, { ctrl: true, name: "u" });
  }
  console.log(JSON.stringify(columns));
} finally {
  rl.close();
  input.destroy();
  output.destroy();
}
