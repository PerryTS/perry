import { ReadableStream } from "node:stream/web";
// Default RS accepts arbitrary value types in enqueue.
const rs = new ReadableStream({
  start(c) {
    c.enqueue(42);
    c.enqueue({ a: 1 });
    c.enqueue([1, 2, 3]);
    c.close();
  },
});
const reader = rs.getReader();
const out: any[] = [];
while (true) {
  const { value, done } = await reader.read();
  if (done) break;
  out.push(typeof value);
}
console.log("types:", out.join(","));
