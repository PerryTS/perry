import { Readable } from "node:stream";
// once(event, fn) — the listener is removed after the event fires once.
const r = new Readable({ read() {} });
let count = 0;
r.once("custom", () => count++);
r.emit("custom");
r.emit("custom");
console.log("listener fired:", count);
console.log("listenerCount after fire:", r.listenerCount("custom"));
