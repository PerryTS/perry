import { Readable } from "node:stream";
// destroy() (no arg) — destroyed becomes true; no 'error' fired.
const r = new Readable({ read() {} });
let errFired = false;
let closeFired = false;
r.on("error", () => (errFired = true));
r.on("close", () => (closeFired = true));
r.destroy();
setImmediate(() => {
  console.log("destroyed:", r.destroyed);
  console.log("error fired:", errFired);
  console.log("close fired:", closeFired);
});
