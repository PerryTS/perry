import { setTimeout, clearTimeout } from "node:timers";
const timeout = setTimeout(() => {}, 100);
const primitive = +timeout;
console.log("primitive type:", typeof primitive);
console.log("primitive positive:", primitive > 0);
clearTimeout(timeout);
