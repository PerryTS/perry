import { compose, Readable, Transform } from "node:stream";
// An error thrown in a middle transform aborts the entire composite.
const src = Readable.from(["a", "b", "c"]);
const bad = new Transform({
  transform(c, _e, cb) {
    if (String(c) === "b") cb(new Error("mid-chain-fail"));
    else cb(null, c);
  },
});
const composite: any = compose(src, bad);
let errMsg: string | null = null;
composite.on("error", (err: any) => (errMsg = err && err.message));
composite.on("data", () => {});
composite.on("close", () => console.log("err received:", errMsg));
