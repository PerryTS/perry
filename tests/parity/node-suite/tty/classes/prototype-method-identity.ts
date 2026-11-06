import * as tty from "node:tty";

// Prototype methods must be stable references across reads — Node caches them
// on the prototype object, so two accesses produce the same function value.
const hasColorsA = tty.WriteStream.prototype.hasColors;
const hasColorsB = tty.WriteStream.prototype.hasColors;
const getDepthA = tty.WriteStream.prototype.getColorDepth;
const getDepthB = tty.WriteStream.prototype.getColorDepth;
const cursorToA = tty.WriteStream.prototype.cursorTo;
const cursorToB = tty.WriteStream.prototype.cursorTo;
const setRawA = tty.ReadStream.prototype.setRawMode;
const setRawB = tty.ReadStream.prototype.setRawMode;

console.log("hasColors identity:", hasColorsA === hasColorsB);
console.log("getColorDepth identity:", getDepthA === getDepthB);
console.log("cursorTo identity:", cursorToA === cursorToB);
console.log("setRawMode identity:", setRawA === setRawB);
console.log("hasColors is function:", typeof hasColorsA === "function");
console.log("getColorDepth is function:", typeof getDepthA === "function");
console.log("cursorTo is function:", typeof cursorToA === "function");
console.log("setRawMode is function:", typeof setRawA === "function");
