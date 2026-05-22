import * as crypto from "node:crypto";

const oneArg = crypto.randomInt(10);
console.log("one arg integer:", Number.isInteger(oneArg));
console.log("one arg range:", oneArg >= 0 && oneArg < 10);

const twoArg = crypto.randomInt(5, 12);
console.log("two arg integer:", Number.isInteger(twoArg));
console.log("two arg range:", twoArg >= 5 && twoArg < 12);

const degenerate = crypto.randomInt(7, 8);
console.log("single value:", degenerate === 7);
