import { inspect } from "node:util";

const sym = Symbol("k");
const obj: any = { [sym]: 1, normal: 2 };
console.log("symbols:", inspect(obj));
console.log("boxed string:", inspect(new String("x")));
console.log("boxed number:", inspect(new Number(3)));
console.log("boxed bool:", inspect(new Boolean(false)));

const boxedString: any = new String("baz");
boxedString.foo = "bar";
const boxedNumber: any = new Number(13.37);
boxedNumber.foo = "bar";
const boxedBool: any = new Boolean(true);
boxedBool.foo = "bar";

console.log("boxed string props:", inspect(boxedString));
console.log("boxed number props:", inspect(boxedNumber));
console.log("boxed bool props:", inspect(boxedBool));
