import { performance } from "node:perf_hooks";

const before = Date.now();
const now = performance.now();
const origin = performance.timeOrigin;
const after = Date.now();
const approxWallClock = origin + now;

console.log("now process relative:", now < 60 * 60 * 1000);
console.log(
  "origin plus now near Date.now:",
  approxWallClock >= before - 5 && approxWallClock <= after + 50,
);
