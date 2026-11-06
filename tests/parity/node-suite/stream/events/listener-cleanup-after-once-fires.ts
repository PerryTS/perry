import { Readable } from "node:stream";
// once() — listener removed cleanly; rawListeners is empty after fire.
const r = new Readable({ read() {} });
r.once("custom", () => {});
const beforeFire = r.rawListeners("custom").length;
r.emit("custom");
const afterFire = r.rawListeners("custom").length;
console.log("before:", beforeFire);
console.log("after:", afterFire);
console.log("cleaned:", afterFire === 0);
