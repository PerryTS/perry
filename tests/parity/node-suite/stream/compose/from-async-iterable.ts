import * as stream from "node:stream";
// stream.compose accepts an async iterable as the first (source) argument.
async function* src() {
  yield "x";
  yield "y";
}
const piped = (stream as any).compose(src());
const out: string[] = [];
piped.on("data", (c: any) => out.push(String(c)));
piped.on("end", () => console.log("joined:", out.join("")));
