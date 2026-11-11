import * as tty from "tty";

console.log("prefixless namespace object:", tty !== null && typeof tty === "object");
console.log("prefixless isatty fn:", typeof tty.isatty === "function");
console.log("prefixless ReadStream fn:", typeof tty.ReadStream === "function");
console.log("prefixless WriteStream fn:", typeof tty.WriteStream === "function");
console.log("prefixless invalid fd false:", tty.isatty(1234567) === false);
