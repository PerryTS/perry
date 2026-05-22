import { inspect } from "node:util";

const sym = Symbol("k");
const obj: any = { [sym]: 1, normal: 2 };
console.log("symbols:", inspect(obj));
console.log("boxed string:", inspect(new String("x")));
console.log("boxed number:", inspect(new Number(3)));
console.log("boxed bool:", inspect(new Boolean(false)));
