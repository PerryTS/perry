import * as tty from "node:tty";

console.log("string zero false:", tty.isatty("0" as any) === false);
console.log("string one false:", tty.isatty("1" as any) === false);
console.log("empty string false:", tty.isatty("" as any) === false);
console.log("nonnumeric string false:", tty.isatty("not-a-fd" as any) === false);
