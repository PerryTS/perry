import { getDefaultHighWaterMark } from "node:stream";
// getDefaultHighWaterMark returns a number for both modes; check both
// arities explicitly (boolean indicates objectMode).
const std = getDefaultHighWaterMark(false);
const obj = getDefaultHighWaterMark(true);
console.log("std is number:", typeof std === "number");
console.log("obj is number:", typeof obj === "number");
console.log("std > 0:", std > 0);
console.log("obj > 0:", obj > 0);
