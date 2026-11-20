import { Console, log, info, debug, warn, error, assert, count, countReset, time, timeEnd, timeLog } from "node:console";

console.log("named Console:", typeof Console);
console.log("named log/info/debug:", typeof log, typeof info, typeof debug);
console.log("named warn/error/assert:", typeof warn, typeof error, typeof assert);
console.log("named count/time:", typeof count, typeof countReset, typeof time, typeof timeEnd, typeof timeLog);
