import { Readable } from "node:stream";
// emit() returns true if at least one listener was called, false if no
// listeners were registered for the event.
const r = new Readable({ read() {} });
const noListeners = r.emit("custom-event");
r.on("custom-event", () => {});
const oneListener = r.emit("custom-event");
console.log("emit no listeners:", noListeners);
console.log("emit one listener:", oneListener);
