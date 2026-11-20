import { getDefaultHighWaterMark } from "node:stream";
// getDefaultHighWaterMark(false) — byte-mode default is 65536 (64KB).
const byteHwm = getDefaultHighWaterMark(false);
console.log("byte hwm:", byteHwm);
console.log("is 65536:", byteHwm === 65536);
