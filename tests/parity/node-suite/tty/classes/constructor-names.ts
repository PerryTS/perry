import * as tty from "node:tty";

console.log("ReadStream name:", tty.ReadStream.name === "ReadStream");
console.log("WriteStream name:", tty.WriteStream.name === "WriteStream");
console.log("ReadStream function type:", typeof tty.ReadStream === "function");
console.log("WriteStream function type:", typeof tty.WriteStream === "function");
