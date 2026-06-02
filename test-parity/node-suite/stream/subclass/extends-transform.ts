import { Transform } from "node:stream";
// Custom Transform subclass implementing _transform.
class Reverse extends Transform {
  _transform(c: any, _e: any, cb: any) {
    cb(null, String(c).split("").reverse().join(""));
  }
}
const t = new Reverse();
const out: string[] = [];
t.on("data", (c) => out.push(String(c)));
t.on("end", () => console.log("joined:", out.join("")));
t.write("ab");
t.end("cd");
