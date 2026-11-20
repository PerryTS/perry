import * as readline from "node:readline";
import { PassThrough } from "node:stream";

const Interface = (readline as any).Interface;
const input = new PassThrough();
if (typeof Interface === "function") {
  const called = Interface({ input, terminal: false });
  const constructed = new Interface({ input, terminal: false });
  console.log(called instanceof Interface, constructed instanceof Interface);
  called.close();
  constructed.close();
} else {
  console.log("missing");
}
input.destroy();
