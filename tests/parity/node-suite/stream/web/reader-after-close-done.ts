import { ReadableStream } from "node:stream/web";
// After the stream closes, further read() calls return { done: true, value: undefined }.
const rs = new ReadableStream({
  start(c) { c.enqueue("x"); c.close(); },
});
const reader = rs.getReader();
const first = await reader.read();
const second = await reader.read();
const third = await reader.read();
console.log("first:", first.value, first.done);
console.log("second:", second.value, second.done);
console.log("third:", third.value, third.done);
