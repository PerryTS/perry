import { createInterface } from "node:readline/promises";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
rl.close();
const result = await rl.question("q> ").catch((error) => error);
console.log(result.name, result.code);
input.destroy();
