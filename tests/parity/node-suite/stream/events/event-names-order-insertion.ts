import { Readable } from "node:stream";
// eventNames() returns events in insertion order (the order in which
// the first listener was registered).
const r = new Readable({ read() {} });
r.on("zeta", () => {});
r.on("alpha", () => {});
r.on("middle", () => {});
const names = r.eventNames();
console.log("names:", names.join(","));
console.log("first is zeta:", names[0] === "zeta");
