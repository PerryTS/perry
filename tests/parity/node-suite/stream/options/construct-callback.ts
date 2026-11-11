import { Readable } from "node:stream";
// The construct(callback) option runs once before the first push/read; the
// callback signals readiness.
let called = false;
const r = new Readable({
  construct(cb) { called = true; cb(); },
  read() {},
});
setImmediate(() => console.log("construct called:", called, " readable:", r.readable));
