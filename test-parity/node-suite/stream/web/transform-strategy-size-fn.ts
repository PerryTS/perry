import { TransformStream } from "node:stream/web";
// TransformStream's writableStrategy can have a custom size() function.
let sizeCalled = 0;
const ts = new TransformStream(undefined, {
  highWaterMark: 10,
  size: (chunk: any) => {
    sizeCalled++;
    return String(chunk).length;
  },
});
const writer = ts.writable.getWriter();
await writer.write("ab");
await writer.write("cdef");
await writer.close();
console.log("size called:", sizeCalled);
console.log("at least 2 calls:", sizeCalled >= 2);
