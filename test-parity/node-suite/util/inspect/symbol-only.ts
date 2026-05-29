import { inspect } from "node:util";

const sym = Symbol("only");
const obj: any = {};
obj[sym] = 42;

console.log(inspect(obj));
