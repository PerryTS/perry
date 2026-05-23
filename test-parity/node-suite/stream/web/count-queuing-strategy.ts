import { CountQueuingStrategy } from "node:stream/web";
// CountQueuingStrategy({ highWaterMark }) — the size of each chunk is 1,
// so highWaterMark gates the chunk count.
const s = new CountQueuingStrategy({ highWaterMark: 5 });
console.log("hwm:", s.highWaterMark);
console.log("size:", s.size("anything"));
