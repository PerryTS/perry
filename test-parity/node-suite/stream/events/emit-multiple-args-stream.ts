import { Readable } from "node:stream";
// emit(event, a, b, c, ...) passes all args to each listener.
const r = new Readable({ read() {} });
let received: any[] = [];
r.on("custom", (a: any, b: any, c: any) => {
  received = [a, b, c];
});
r.emit("custom", 1, "two", { three: 3 });
console.log("args:", JSON.stringify(received));
