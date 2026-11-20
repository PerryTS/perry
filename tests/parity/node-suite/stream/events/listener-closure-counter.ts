import { Readable } from "node:stream";
// Listener uses outer-scope counter; counter increments on each fire.
const r = new Readable({ read() {} });
let counter = 0;
r.on("ping", () => counter++);
r.emit("ping");
r.emit("ping");
r.emit("ping");
console.log("count:", counter);
