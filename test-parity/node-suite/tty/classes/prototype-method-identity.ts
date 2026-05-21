import * as tty from "node:tty";

// Prototype methods must be stable references across reads — Node caches them
// on the prototype object, so two accesses produce the same function value.
const hasColorsA = tty.WriteStream.prototype.hasColors;
const hasColorsB = tty.WriteStream.prototype.hasColors;
const getDepthA = tty.WriteStream.prototype.getColorDepth;
const getDepthB = tty.WriteStream.prototype.getColorDepth;

console.log("hasColors identity:", hasColorsA === hasColorsB);
console.log("getColorDepth identity:", getDepthA === getDepthB);
console.log("hasColors is function:", typeof hasColorsA === "function");
console.log("getColorDepth is function:", typeof getDepthA === "function");
