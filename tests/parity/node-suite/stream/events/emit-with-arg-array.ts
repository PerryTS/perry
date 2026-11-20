import { Readable } from "node:stream";
// emit(event, ...args) — multiple args passed to listeners individually.
const r = new Readable({ read() {} });
let captured: any = null;
r.on("multi", (a: any, b: any, c: any, d: any) => {
  captured = { a, b, c, d };
});
r.emit("multi", 1, "two", { three: 3 }, [4, 5]);
console.log(JSON.stringify(captured));
