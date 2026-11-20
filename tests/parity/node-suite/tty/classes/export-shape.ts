import * as tty from "node:tty";

console.log("ReadStream function:", typeof tty.ReadStream === "function");
console.log("WriteStream function:", typeof tty.WriteStream === "function");
// Constructor instantiation/prototype behavior requires a real TTY-backed fd
// and is intentionally left out of this non-interactive parity shard.
