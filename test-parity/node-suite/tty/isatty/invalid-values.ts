import * as tty from "node:tty";

console.log("large fd false:", tty.isatty(1234567) === false);
console.log("negative fd false:", tty.isatty(-1) === false);
console.log("fractional fd false:", tty.isatty(0.5) === false && tty.isatty(1.3) === false);
console.log("string fd false:", tty.isatty("abc" as any) === false);
console.log("object fd false:", tty.isatty({} as any) === false);
console.log("array fd false:", tty.isatty([] as any) === false);
console.log("nullish fd false:", tty.isatty(null as any) === false && tty.isatty(undefined as any) === false);
