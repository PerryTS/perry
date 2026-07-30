import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const output = new PassThrough();
const first = createInterface({ input, output, terminal: true });
let second: ReturnType<typeof createInterface> | undefined;
try {
  const lines = await new Promise<string[]>((resolve) => {
    first.once("line", (firstLine) => {
      first.close();
      second = createInterface({ input, output, terminal: true });
      second.once("line", (secondLine) => resolve([firstLine, secondLine]));
      input.write(Buffer.from([0x98, 0x83]));
      input.write("bar\n");
    });
    input.write(Buffer.concat([Buffer.from("foo\n"), Buffer.from([0xe2])]));
  });
  console.log(JSON.stringify(lines));
} finally {
  first.close();
  second?.close();
  input.destroy();
  output.destroy();
}
