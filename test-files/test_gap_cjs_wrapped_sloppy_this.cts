module.exports = {};
function sloppy() { return this; }
function strict() { "use strict"; return this; }
console.log("sloppy", sloppy.call(undefined) === globalThis, sloppy.call(null) === globalThis);
console.log("strict", strict.call(undefined) === undefined, strict.call(null) === null);
console.log("primitive", typeof sloppy.call(1), sloppy.call(1) instanceof Number);
const receiver = {};
console.log("object", sloppy.call(receiver) === receiver);
const expression = function () { return this === globalThis; };
console.log("expression", expression.call(undefined));
function nested() { return (() => this)() === this && this === globalThis; }
console.log("arrow", nested.call(undefined));
const strictModule = require("./gap_cjs_wrapped_strict_receiver.cjs");
console.log("strict-module", strictModule.call(undefined) === undefined, strictModule.call(7) === 7);
