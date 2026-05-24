import { ReadableStream } from "node:stream/web";
// cancel(undefined) explicitly — reason is undefined in cancel hook.
let seen: any = "untouched";
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); },
  cancel(reason) { seen = reason; },
});
await rs.cancel(undefined);
console.log("seen:", seen);
console.log("is undefined:", seen === undefined);
