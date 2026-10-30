// `new C(...args)` on an imported class passes the spread elements as
// separate constructor arguments, not one array argument.
import { Path, Plain } from "./_helpers/imported_class_spread_new/classes.ts";

const one = ["kind"];
const two = ["a", "b"];

const plainOne = new Plain(...one);
console.log(plainOne.count, plainOne.first);
const plainMixed = new Plain(...one, "x");
console.log(plainMixed.count, plainMixed.first);

const pathOne = new Path(...one);
console.log(pathOne.length, typeof pathOne[0], JSON.stringify(pathOne));
const pathTwo = new Path(...two);
console.log(pathTwo.length, JSON.stringify(pathTwo));
const pathNone = new Path(...[]);
console.log(pathNone.length);
