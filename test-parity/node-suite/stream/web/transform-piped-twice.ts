import { ReadableStream, TransformStream } from "node:stream/web";
// pipeThrough then pipeThrough again.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const t1 = new TransformStream({ transform(c, ctrl) { ctrl.enqueue(String(c) + "1"); } });
const t2 = new TransformStream({ transform(c, ctrl) { ctrl.enqueue(String(c) + "2"); } });
const result = rs.pipeThrough(t1).pipeThrough(t2);
const reader = result.getReader();
const { value } = await reader.read();
console.log("result:", value);
console.log("is x12:", value === "x12");
