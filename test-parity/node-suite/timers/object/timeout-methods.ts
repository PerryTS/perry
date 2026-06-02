import { setTimeout, clearTimeout } from "node:timers";
const timeout = setTimeout(() => {}, 100);
console.log("hasRef value:", timeout.hasRef());
console.log("unref same:", timeout.unref() === timeout);
console.log("hasRef after unref:", timeout.hasRef());
console.log("ref same:", timeout.ref() === timeout);
console.log("refresh same:", timeout.refresh() === timeout);
clearTimeout(timeout);
