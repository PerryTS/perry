import { compose, Readable } from "node:stream";
// An async-generator handler that throws — the composite emits 'error'.
async function* failing(_src: AsyncIterable<any>) {
  yield "a";
  throw new Error("handler-fail");
}
const composite: any = compose(Readable.from(["x"]), failing);
let errMsg: string | null = null;
composite.on("error", (e: any) => (errMsg = e && e.message));
composite.on("data", () => {});
composite.on("close", () => console.log("err:", errMsg));
