import { Buffer } from "node:buffer";

const fromArray = Buffer.from([-1, -2, 255, 256, 511]);
const fromOf = Buffer.of(-1, -2, 255, 256, 511);
console.log("from array wrap:", fromArray.toString("hex"));
console.log("of wrap:", fromOf.toString("hex"));
