import { setTimeout, clearTimeout } from "node:timers";
const timeout = setTimeout(() => {}, 100);
console.log("initial hasRef:", timeout.hasRef());
console.log("unref same:", timeout.unref() === timeout);
console.log("after unref:", timeout.hasRef());
console.log("ref same:", timeout.ref() === timeout);
console.log("after ref:", timeout.hasRef());
clearTimeout(timeout);
