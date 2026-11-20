import { setInterval } from "node:timers/promises";

const start = Date.now();
const it = setInterval(40, "tick");

const first = await it.next();
const firstElapsed = Date.now() - start;
const second = await it.next();
const secondElapsed = Date.now() - start;
const returned = it.return ? await it.return() : undefined;
const afterReturn = await it.next();

console.log("first value:", first.value);
console.log("first done:", first.done);
console.log("first delayed:", firstElapsed >= 30);
console.log("second value:", second.value);
console.log("second done:", second.done);
console.log("second delayed:", secondElapsed >= 70);
console.log("return done:", returned?.done === true);
console.log("after return done:", afterReturn.done);
