import { EventEmitter, once } from "node:events";

const em = new EventEmitter();
const seen: unknown[] = [];

em.on("data", function (a: unknown, b: unknown, c: unknown) {
  seen.push([arguments.length, a, b, c, this === em]);
});

em.once("data", function (a: unknown, b: unknown, c: unknown) {
  seen.push(["once", arguments.length, a, b, c, this === em]);
});

const p = once(em, "data");
console.log("emit ret:", em.emit("data", "a", 2, true));
console.log("seen first:", JSON.stringify(seen));
console.log("once promise:", JSON.stringify(await p));

console.log("emit second ret:", em.emit("data", "b", 3, false));
console.log("seen second:", JSON.stringify(seen));
