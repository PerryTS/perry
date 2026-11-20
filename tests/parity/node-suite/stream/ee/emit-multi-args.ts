import { PassThrough } from "node:stream";
// emit(event, a, b, c) passes ALL trailing args to listeners.
const p = new PassThrough();
let saw = "";
p.on("custom", (a: string, b: string, c: string) => { saw = `${a}|${b}|${c}`; });
(p as any).emit("custom", "one", "two", "three");
console.log("args:", saw);
