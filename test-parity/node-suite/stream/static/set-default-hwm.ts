import { setDefaultHighWaterMark, getDefaultHighWaterMark } from "node:stream";
// setDefaultHighWaterMark(objectMode, value) updates the platform default
// returned by getDefaultHighWaterMark.
console.log("set is function:", typeof setDefaultHighWaterMark === "function");
console.log("get is function:", typeof getDefaultHighWaterMark === "function");
