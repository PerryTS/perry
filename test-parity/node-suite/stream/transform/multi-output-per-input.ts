import { Transform } from "node:stream";
// A transform can push multiple outputs per single input by calling
// this.push() multiple times before invoking cb().
const splitter = new Transform({
  transform(chunk, _enc, cb) {
    for (const c of String(chunk)) this.push(c);
    cb();
  },
});
const out: string[] = [];
splitter.on("data", (c) => out.push(String(c)));
splitter.on("end", () => console.log("split:", out.join(",")));
splitter.write("abc");
splitter.end();
