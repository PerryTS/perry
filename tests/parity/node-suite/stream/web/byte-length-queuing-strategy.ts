import { ByteLengthQueuingStrategy } from "node:stream/web";
// ByteLengthQueuingStrategy measures each chunk by its byteLength.
const s = new ByteLengthQueuingStrategy({ highWaterMark: 1024 });
console.log("hwm:", s.highWaterMark);
console.log("size of buffer-3:", s.size(Buffer.from("abc")));
