import { PassThrough } from "node:stream";
// PassThrough pipes input chunks straight through to readers.
const p = new PassThrough();
let received = "";
p.on("data", (chunk) => {
  received += String(chunk);
});
p.on("end", () => console.log("received:", received));
p.write("hello ");
p.write("world");
p.end();
