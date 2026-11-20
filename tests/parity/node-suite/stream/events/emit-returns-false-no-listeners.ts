import { Readable } from "node:stream";
// emit('event') with no listeners returns false; with listeners returns true.
const r = new Readable({ read() {} });
const noListenerResult = r.emit("no-listener-here");
r.on("present", () => {});
const withListenerResult = r.emit("present");
console.log("no-listener:", noListenerResult);
console.log("with-listener:", withListenerResult);
console.log("correct:", noListenerResult === false && withListenerResult === true);
