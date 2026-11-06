import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
const consumed = (async () => {
  try {
    for await (const line of rl) console.log(line);
    console.log("done");
  } catch (error: any) {
    console.log(error === failure, error.message);
  }
})();
const failure = new Error("boom");
input.destroy(failure);
await consumed;
