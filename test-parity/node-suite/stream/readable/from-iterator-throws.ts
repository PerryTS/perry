import { Readable } from "node:stream";
// Iterator that throws on next() — Readable.from emits 'error'.
const throwingIter = {
  [Symbol.iterator]() {
    return {
      next() {
        throw new Error("iter-fail");
      },
    };
  },
};
const r = Readable.from(throwingIter as any);
let errMsg: string | null = null;
r.on("error", (e) => (errMsg = e && e.message));
r.on("data", () => {});
setImmediate(() => console.log("err:", errMsg));
