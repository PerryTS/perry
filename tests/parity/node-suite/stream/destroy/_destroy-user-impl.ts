import { Readable } from "node:stream";
// User _destroy(err, cb) is called during destroy() with the error and a cb.
let userCalled = false;
let receivedErr: any = null;
const r = new Readable({
  read() {},
  destroy(err: any, cb: any) {
    userCalled = true;
    receivedErr = err;
    cb(err);
  },
});
r.on("error", () => {});
r.destroy(new Error("user-destroy-test"));
setImmediate(() => {
  console.log("user impl called:", userCalled);
  console.log("err message:", receivedErr && (receivedErr as Error).message);
});
