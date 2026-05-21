import * as tty from "node:tty";

// Node exposes real constructor prototypes even when no TTY-backed instance is
// created. Deno also relies on WriteStream.prototype for color helpers.
console.log("ReadStream prototype object:", tty.ReadStream.prototype !== null && typeof tty.ReadStream.prototype === "object");
console.log("WriteStream prototype object:", tty.WriteStream.prototype !== null && typeof tty.WriteStream.prototype === "object");
console.log("ReadStream constructor link:", tty.ReadStream.prototype?.constructor === tty.ReadStream);
console.log("WriteStream constructor link:", tty.WriteStream.prototype?.constructor === tty.WriteStream);
