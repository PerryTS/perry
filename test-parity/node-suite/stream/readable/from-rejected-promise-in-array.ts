import { Readable } from "node:stream";
// Readable.from([Promise.resolve(1), Promise.reject(err), Promise.resolve(3)])
// — emits error when it encounters the rejection.
const r = Readable.from([
  Promise.resolve(1),
  Promise.reject(new Error("inner-fail")),
  Promise.resolve(3),
]);
let errMsg: string | null = null;
r.on("error", (e) => (errMsg = e && e.message));
const out: any[] = [];
r.on("data", (v) => out.push(v));
setImmediate(() => {
  setImmediate(() => {
    console.log("got data count:", out.length);
    console.log("err:", errMsg);
  });
});
