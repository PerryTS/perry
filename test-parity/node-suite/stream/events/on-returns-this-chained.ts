import { Readable } from "node:stream";
// on() returns the emitter itself, enabling chained registrations.
const r = new Readable({ read() {} });
const chained = r.on("data", () => {}).on("end", () => {});
console.log("chained is r:", chained === r);
console.log("data listeners:", r.listenerCount("data"));
console.log("end listeners:", r.listenerCount("end"));
