import { WritableStream } from "node:stream/web";
// abort() with no arg — abort succeeds; reason is undefined.
let seen: any = "untouched";
const ws = new WritableStream({
  write() {},
  abort(reason) { seen = reason; },
});
await ws.abort();
console.log("sink reason:", seen);
console.log("undefined:", seen === undefined);
