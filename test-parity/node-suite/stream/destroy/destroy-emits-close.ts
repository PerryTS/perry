import { Readable } from "node:stream";
// destroy() asynchronously emits the 'close' event.
const r = new Readable({ read() {} });
let closeFired = false;
r.on("close", () => (closeFired = true));
r.destroy();
setImmediate(() => console.log("close fired:", closeFired));
