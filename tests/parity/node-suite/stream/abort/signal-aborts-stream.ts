import * as stream from "node:stream";
import { Readable } from "node:stream";
// stream.addAbortSignal(signal, stream): when the controller fires abort,
// the stream emits 'error' with the AbortError.
const ctrl = new AbortController();
const r = new Readable({ read() {} });
(stream as any).addAbortSignal(ctrl.signal, r);
let errored = false;
r.on("error", () => (errored = true));
ctrl.abort();
setImmediate(() => console.log("errored on abort:", errored));
