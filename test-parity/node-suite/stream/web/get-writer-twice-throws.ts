import { WritableStream } from "node:stream/web";
// Second getWriter() on locked WritableStream throws TypeError.
const ws = new WritableStream({ write() {} });
const _w1 = ws.getWriter();
let caught: string | null = null;
try {
  ws.getWriter();
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
console.log("locked:", ws.locked);
