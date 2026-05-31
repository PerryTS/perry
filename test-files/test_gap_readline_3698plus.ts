import { createInterface } from "node:readline";
import * as readline from "node:readline";
import { connect } from "node:tls";
import * as tls from "node:tls";
import { Readable } from "node:stream";
import * as readlinePromises from "node:readline/promises";

function shape(value: any): string {
  if (value === null) return "null";
  const type = typeof value;
  if (type === "function") return "function";
  if (type === "object") return "object";
  return type;
}

// #3698 named-export resolution: createInterface + tls.connect must be
// function-valued for both named and namespace imports (matching Node).
console.log("readline named createInterface:", shape(createInterface));
console.log("readline ns createInterface:", shape(readline.createInterface));
console.log("tls named connect:", shape(connect));
console.log("tls ns connect:", shape(tls.connect));

// #3212 node:readline/promises: createInterface is callable and returns an
// Interface whose question() yields a Promise; close is callable.
console.log("rlp createInterface:", shape(readlinePromises.createInterface));
const rlp = readlinePromises.createInterface({ input: Readable.from(["y\n"]), terminal: false });
console.log("rlp.question:", shape(rlp.question));
console.log("rlp.close:", shape(rlp.close));
const q = rlp.question("prompt? ");
console.log("rlp.question returns promise:", shape(q) === "object" && shape((q as any).then) === "function");
rlp.close();
