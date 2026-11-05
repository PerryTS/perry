import * as timersPromises from "node:timers/promises";
console.log("promises namespace:", typeof timersPromises);
console.log("setTimeout:", typeof timersPromises.setTimeout);
console.log("setImmediate:", typeof timersPromises.setImmediate);
console.log("setInterval:", typeof timersPromises.setInterval);
