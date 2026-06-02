import { Readable, PassThrough } from "node:stream";
// Error handler on source — when src errors, the handler fires.
let srcErrMsg: string | null = null;
const src = new Readable({ read() {} });
src.on("error", (e) => (srcErrMsg = e && e.message));
const dst = new PassThrough();
dst.on("data", () => {});
dst.on("error", () => {});
src.pipe(dst);
src.destroy(new Error("src-error"));
setImmediate(() => {
  setImmediate(() => console.log("src err:", srcErrMsg));
});
