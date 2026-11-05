import tty from "node:tty";

console.log("default is object:", tty !== null && typeof tty === "object");
console.log("default isatty invalid false:", tty.isatty(1234567) === false);
console.log("default isatty string false:", tty.isatty("abc" as any) === false);
console.log("default classes fn:", typeof tty.ReadStream === "function" && typeof tty.WriteStream === "function");
