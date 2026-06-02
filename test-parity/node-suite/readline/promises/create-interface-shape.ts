import * as readlinePromises from "node:readline/promises";
import { Readable } from "node:stream";

const rl = readlinePromises.createInterface({
  input: Readable.from(["answer\n"]),
  terminal: false,
});

console.log("createInterface type:", typeof rl);
console.log("close type:", typeof rl.close);
console.log("question type:", typeof rl.question);
console.log("close return:", rl.close());
