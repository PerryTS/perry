import { Readable } from "node:stream";
// destroy(err) emits 'error' event with the supplied error before 'close'.
const r = new Readable({ read() {} });
let errFired = false;
let closeFired = false;
let order = "";
r.on("error", () => {
  errFired = true;
  order += "E";
});
r.on("close", () => {
  closeFired = true;
  order += "C";
});
r.destroy(new Error("boom"));
setImmediate(() => {
  console.log("err fired:", errFired);
  console.log("close fired:", closeFired);
  console.log("order:", order);
});
