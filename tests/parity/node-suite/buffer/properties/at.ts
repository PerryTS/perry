import { Buffer } from "node:buffer";

const b = Buffer.from([10, 20, 30]);
console.log("at 0:", b.at(0));
console.log("at -1:", b.at(-1));
console.log("at oob:", b.at(3));
console.log("at neg oob:", b.at(-4));
