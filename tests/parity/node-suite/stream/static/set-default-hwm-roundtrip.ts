import { setDefaultHighWaterMark, getDefaultHighWaterMark } from "node:stream";
// setDefaultHighWaterMark(false, N) then getDefaultHighWaterMark(false) === N.
const original = getDefaultHighWaterMark(false);
setDefaultHighWaterMark(false, 32768);
const updated = getDefaultHighWaterMark(false);
console.log("updated:", updated);
console.log("matches:", updated === 32768);
// restore
setDefaultHighWaterMark(false, original);
