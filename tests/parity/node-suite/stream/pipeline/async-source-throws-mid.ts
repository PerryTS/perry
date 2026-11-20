import { PassThrough, pipeline } from "node:stream";
// pipeline(asyncGenSrc, dst, cb) — asyncGen throws mid → cb receives error.
async function* gen() {
  yield "a";
  throw new Error("src-fail");
}
const dst = new PassThrough();
dst.on("data", () => {});
let errMsg: string | null = null;
pipeline(gen(), dst, (err: any) => {
  errMsg = err && err.message;
  console.log("err:", errMsg);
});
