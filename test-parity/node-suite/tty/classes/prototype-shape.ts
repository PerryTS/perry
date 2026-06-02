import * as tty from "node:tty";

// Node exposes real constructor prototypes even when no TTY-backed instance is
// created.
console.log("ReadStream prototype object:", tty.ReadStream.prototype !== null && typeof tty.ReadStream.prototype === "object");
console.log("WriteStream prototype object:", tty.WriteStream.prototype !== null && typeof tty.WriteStream.prototype === "object");
console.log("ReadStream constructor link:", tty.ReadStream.prototype?.constructor === tty.ReadStream);
console.log("WriteStream constructor link:", tty.WriteStream.prototype?.constructor === tty.WriteStream);
console.log("ReadStream setRawMode:", typeof tty.ReadStream.prototype?.setRawMode === "function");
console.log("WriteStream isTTY:", tty.WriteStream.prototype?.isTTY === true);
for (const name of ["getColorDepth", "hasColors", "_refreshSize", "cursorTo", "moveCursor", "clearLine", "clearScreenDown", "getWindowSize"]) {
  console.log("WriteStream prototype " + name + ":", typeof (tty.WriteStream.prototype as any)[name] === "function");
}
