import { ReadableStream, TransformStream } from "node:stream/web";
// TransformStream's flush(controller) runs after the final input chunk so
// it can emit trailing data before the readable side closes.
const ts = new TransformStream({
  transform(c, ctl) { ctl.enqueue(c); },
  flush(ctl) { ctl.enqueue("<TAIL>"); },
});
const src = new ReadableStream({
  start(c) { c.enqueue("a"); c.close(); },
});
const out: any[] = [];
const reader = src.pipeThrough(ts).getReader();
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(value);
}
console.log("joined:", out.join(""));
