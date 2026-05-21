import * as tty from "node:tty";

console.log("fd0 boolean:", typeof tty.isatty(0) === "boolean");
console.log("fd1 boolean:", typeof tty.isatty(1) === "boolean");
console.log("fd2 boolean:", typeof tty.isatty(2) === "boolean");
console.log("fd0 false in harness:", tty.isatty(0) === false);
console.log("fd1 false in harness:", tty.isatty(1) === false);
console.log("fd2 false in harness:", tty.isatty(2) === false);
