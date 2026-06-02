import { Readable, addAbortSignal } from "node:stream";
// addAbortSignal(signal, stream) returns the stream.
const ctrl = new AbortController();
const r = new Readable({ read() {} });
const returned = addAbortSignal(ctrl.signal, r);
console.log("returns stream:", returned === r);
r.on("error", () => {});
