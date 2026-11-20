import * as tty from "node:tty";

console.log("fd3 false:", tty.isatty(3) === false);
console.log("negative zero false:", tty.isatty(-0) === false);
console.log("nan false:", tty.isatty(NaN) === false);
console.log("positive infinity false:", tty.isatty(Infinity) === false);
console.log("negative infinity false:", tty.isatty(-Infinity) === false);
