import { Readable } from "node:stream";

const r = new Readable({ read() {} });
const h = () => {};
const g = () => {};

r.on("x", h);
r.on("x", h);
r.on("x", g);

console.log("total:", r.listenerCount("x"));
console.log("h count:", r.listenerCount("x", h));
console.log("g count:", r.listenerCount("x", g));
