import { Transform } from "node:stream";
// flush(cb) runs once after the final chunk so the transform can emit
// trailing data before 'end'.
const t = new Transform({
  transform(c, _e, cb) { cb(null, c); },
  flush(cb) { cb(null, "<TAIL>"); },
});
const out: string[] = [];
t.on("data", (c) => out.push(String(c)));
t.on("end", () => console.log("joined:", out.join("")));
t.write("a");
t.write("b");
t.end();
