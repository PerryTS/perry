import { randomInt } from "node:crypto";

const n = randomInt(2, 6);
console.log("named integer:", Number.isInteger(n));
console.log("named range:", n >= 2 && n < 6);
