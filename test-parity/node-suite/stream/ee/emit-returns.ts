import { PassThrough } from "node:stream";
// emit('evt') returns true when there's at least one listener, false otherwise.
const p = new PassThrough();
console.log("no listener:", (p as any).emit("custom"));
p.on("custom", () => {});
console.log("with listener:", (p as any).emit("custom"));
