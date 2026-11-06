import { getDefaultHighWaterMark } from "node:stream";
// getDefaultHighWaterMark(true) — objectMode default is 16.
const objHwm = getDefaultHighWaterMark(true);
console.log("objectMode hwm:", objHwm);
console.log("is 16:", objHwm === 16);
