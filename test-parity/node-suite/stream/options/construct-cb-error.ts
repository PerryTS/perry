import { Readable } from "node:stream";
// construct(cb) calling cb(err) propagates the error via the 'error' event
// instead of continuing initialisation.
let msg = "";
const r = new Readable({
  construct(cb) { cb(new Error("init-failed")); },
  read() {},
});
r.on("error", (e) => (msg = (e as Error).message));
setImmediate(() => console.log("got:", msg));
