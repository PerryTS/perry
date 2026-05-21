import * as tty from "node:tty";

console.log("stdin isTTY matches:", process.stdin.isTTY === true ? tty.isatty(0) === true : process.stdin.isTTY === undefined);
console.log("stdout isTTY matches:", process.stdout.isTTY === true ? tty.isatty(1) === true : process.stdout.isTTY === undefined);
console.log("stderr isTTY matches:", process.stderr.isTTY === true ? tty.isatty(2) === true : process.stderr.isTTY === undefined);
console.log("stdout columns type:", typeof process.stdout.columns === "number" || typeof process.stdout.columns === "undefined");
console.log("stdout rows type:", typeof process.stdout.rows === "number" || typeof process.stdout.rows === "undefined");
console.log("stdout color depth fn type:", typeof process.stdout.getColorDepth === "function" || typeof process.stdout.getColorDepth === "undefined");
console.log("stdout has colors fn type:", typeof process.stdout.hasColors === "function" || typeof process.stdout.hasColors === "undefined");
