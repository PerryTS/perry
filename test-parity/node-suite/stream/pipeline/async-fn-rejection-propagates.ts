import { Readable, pipeline } from "node:stream";
// async-fn sink that throws — pipeline cb receives the error.
const src = Readable.from(["x"]);
let errMsg: string | null = null;
pipeline(
  src,
  async function (_s: AsyncIterable<any>) {
    throw new Error("sink-fn-fail");
  },
  (err: any) => {
    errMsg = err && err.message;
    console.log("err:", errMsg);
  },
);
