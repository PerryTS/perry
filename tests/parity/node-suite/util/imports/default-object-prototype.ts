// The default export is module.exports, an ordinary object that inherits the
// Object.prototype methods (#13963).
import util from "node:util";
import fs from "node:fs";
import os from "node:os";

console.log("util.hasOwnProperty:", typeof util.hasOwnProperty);
console.log("util own inspect:", util.hasOwnProperty("inspect"));
console.log("util own missing:", util.hasOwnProperty("noSuchExport"));
console.log("util enumerable inspect:", util.propertyIsEnumerable("inspect"));
console.log("fs own readFileSync:", fs.hasOwnProperty("readFileSync"));
console.log("os own arch:", os.hasOwnProperty("arch"));
console.log("Object.prototype in chain:", Object.prototype.isPrototypeOf(util));
