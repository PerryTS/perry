import { Readable } from "node:stream";
// new Readable() with no read() option throws when read() is called (the
// default _read fires ERR_METHOD_NOT_IMPLEMENTED via 'error').
const r = new Readable();
let errored = false;
let seen: any;
r.on("error", (err) => {
  errored = true;
  seen = err;
});
r.read();
setImmediate(() => {
  console.log("errored:", errored);
  console.log("instanceof Error:", seen instanceof Error);
  console.log("code:", seen?.code);
  console.log("message:", seen?.message);
});
