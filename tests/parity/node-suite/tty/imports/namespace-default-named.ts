import * as ttyNS from "node:tty";
import ttyDefault from "node:tty";
import { isatty, ReadStream, WriteStream } from "node:tty";

console.log("namespace object:", ttyNS !== null && typeof ttyNS === "object");
console.log("namespace isatty fn:", typeof ttyNS.isatty === "function");
console.log("default object:", ttyDefault !== null && typeof ttyDefault === "object");
console.log("default isatty fn:", typeof ttyDefault.isatty === "function");
console.log("named isatty fn:", typeof isatty === "function");
console.log("named classes fn:", typeof ReadStream === "function" && typeof WriteStream === "function");
