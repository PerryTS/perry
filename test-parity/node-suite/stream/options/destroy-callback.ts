import { Readable } from "node:stream";
// The destroy(err, cb) option lets user code clean up; cb finalizes destruction.
let called = false;
const r = new Readable({
  read() {},
  destroy(_err, cb) { called = true; cb(null); },
});
r.destroy();
setImmediate(() => console.log("destroy callback ran:", called));
