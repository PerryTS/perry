import { getDefaultHighWaterMark } from "node:stream";
// getDefaultHighWaterMark(objectMode) returns the platform default
// (16384 for byte streams, 16 for objectMode).
console.log("byte:", getDefaultHighWaterMark(false));
console.log("object:", getDefaultHighWaterMark(true));
