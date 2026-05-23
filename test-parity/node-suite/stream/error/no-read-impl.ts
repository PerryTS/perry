import { Readable } from "node:stream";
// new Readable() with no read() option throws when read() is called (the
// default _read fires ERR_METHOD_NOT_IMPLEMENTED via 'error').
const r = new Readable();
let errored = false;
r.on("error", () => (errored = true));
r.read();
setImmediate(() => console.log("errored:", errored));
