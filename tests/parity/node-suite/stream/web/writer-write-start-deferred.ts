import { WritableStream } from "node:stream/web";

let called = false;
const ws = new WritableStream({
  write() {
    called = true;
  },
});

const writer = ws.getWriter();
const p = writer.write("x");
console.log("called before await:", called);
await p;
console.log("called after await:", called);
