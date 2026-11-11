import { WritableStream } from "node:stream/web";
// Sink object can use arrow functions for methods (no `this`).
let written: string | null = null;
const ws = new WritableStream({
  write: (c) => {
    written = String(c);
  },
});
const w = ws.getWriter();
await w.write("hi");
await w.close();
console.log("written:", written);
