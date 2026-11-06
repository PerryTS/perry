import { Readable } from "node:stream";
import { errorMonitor } from "node:events";
// errorMonitor is a symbol that lets you observe 'error' events WITHOUT
// consuming them. Without the symbol you can't be both observer + listener.
const r = new Readable({ read() {} });
let observed = false;
let consumed = false;
r.on(errorMonitor, () => (observed = true));
r.on("error", () => (consumed = true));
r.destroy(new Error("watcher-test"));
setImmediate(() => {
  console.log("observed:", observed);
  console.log("consumed:", consumed);
});
