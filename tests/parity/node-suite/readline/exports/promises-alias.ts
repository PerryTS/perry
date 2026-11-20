import * as readline from "node:readline";
import * as promises from "node:readline/promises";

console.log(readline.promises === promises);
console.log(readline.Interface === promises.Interface);
console.log(readline.createInterface === promises.createInterface);
