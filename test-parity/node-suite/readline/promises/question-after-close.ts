import { createInterface } from "node:readline/promises";
import { PassThrough } from "node:stream";

const input = new PassThrough();
const rl = createInterface({ input, terminal: false });
rl.close();
if (typeof (rl as any).on === "function") {
  const result = await rl.question("q> ").catch((error) => error);
  console.log(result.name, result.code);
} else {
  console.log("missing");
}
input.destroy();
