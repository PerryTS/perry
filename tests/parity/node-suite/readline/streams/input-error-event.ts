import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
const failure = new Error("boom");
let same = false;
input.on("error", () => {});
rl.on("error", (error) => same = error === failure);
input.destroy(failure);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(same);
rl.close();
