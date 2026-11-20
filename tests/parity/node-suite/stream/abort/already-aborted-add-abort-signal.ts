import * as stream from "node:stream";
import { Readable } from "node:stream";

const ctrl = new AbortController();
ctrl.abort();

const r = new Readable({ read() {} });
let name = "";
r.on("error", (err) => (name = (err as Error).name));

(stream as any).addAbortSignal(ctrl.signal, r);

setImmediate(() => {
  console.log("pre-aborted error:", name);
  console.log("destroyed:", r.destroyed);
});
