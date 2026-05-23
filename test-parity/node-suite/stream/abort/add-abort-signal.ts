import * as stream from "node:stream";
import { Readable } from "node:stream";
// stream.addAbortSignal(signal, stream) wires an AbortController to abort
// the stream when the signal fires.
const ctrl = new AbortController();
const r = new Readable({ read() {} });
const wrapped = (stream as any).addAbortSignal(ctrl.signal, r);
console.log("returned same stream:", wrapped === r);
