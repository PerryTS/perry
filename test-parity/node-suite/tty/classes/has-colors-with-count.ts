import tty from "node:tty";

// Node's WriteStream.prototype.hasColors accepts an optional color count.
// hasColors() / hasColors(count) / hasColors(count, env) must each return a boolean
// without throwing for the documented arities.
console.log("hasColors() boolean:", typeof tty.WriteStream.prototype.hasColors() === "boolean");
console.log("hasColors(2) boolean:", typeof tty.WriteStream.prototype.hasColors(2) === "boolean");
console.log("hasColors(16) boolean:", typeof tty.WriteStream.prototype.hasColors(16) === "boolean");
console.log("hasColors(256) boolean:", typeof tty.WriteStream.prototype.hasColors(256) === "boolean");
console.log("hasColors(16777216) boolean:", typeof tty.WriteStream.prototype.hasColors(16777216) === "boolean");
