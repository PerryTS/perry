import { ReadableStream, WritableStream } from "node:stream/web";
// .locked starts false on a freshly-constructed stream; flips true after
// getReader()/getWriter().
const rs = new ReadableStream();
const ws = new WritableStream();
console.log("rs initial locked:", rs.locked);
console.log("ws initial locked:", ws.locked);
const _r = rs.getReader();
const _w = ws.getWriter();
console.log("rs after getReader:", rs.locked);
console.log("ws after getWriter:", ws.locked);
