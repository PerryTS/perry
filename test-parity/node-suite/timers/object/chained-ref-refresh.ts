import { setTimeout, clearTimeout } from "node:timers";
const timeout = setTimeout(() => {}, 100);
console.log("chain same:", timeout.unref().ref().refresh() === timeout);
console.log("chain hasRef:", timeout.hasRef());
clearTimeout(timeout);
