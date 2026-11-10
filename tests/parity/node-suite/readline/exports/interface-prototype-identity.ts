import * as readline from "node:readline";
import { PassThrough } from "node:stream";

const Interface = (readline as any).Interface;
const input = new PassThrough();
if (typeof Interface === "function") {
  const rl = readline.createInterface({ input, terminal: false });
  console.log(Object.getPrototypeOf(rl) === Interface.prototype);
  rl.close();
} else {
  console.log("missing");
}
input.destroy();
