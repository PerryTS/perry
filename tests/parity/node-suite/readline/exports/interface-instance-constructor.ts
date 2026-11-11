import * as readline from "node:readline";
import { PassThrough } from "node:stream";

const Interface = (readline as any).Interface;
const input = new PassThrough();
if (typeof Interface === "function") {
  const rl = readline.createInterface({ input, terminal: false });
  console.log(rl.constructor === Interface);
  rl.close();
} else {
  console.log("missing");
}
input.destroy();
