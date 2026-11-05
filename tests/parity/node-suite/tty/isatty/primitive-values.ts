import * as tty from "node:tty";

console.log("true false:", tty.isatty(true as any) === false);
console.log("false false:", tty.isatty(false as any) === false);
console.log("null false:", tty.isatty(null as any) === false);
console.log("undefined false:", tty.isatty(undefined as any) === false);
