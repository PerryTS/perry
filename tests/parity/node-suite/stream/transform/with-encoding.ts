import { Transform } from "node:stream";
// A Transform respects encoding when writing strings; chunks reach the
// transform handler as Buffers (default) or strings (when decodeStrings:false).
const out: string[] = [];
const t = new Transform({
  defaultEncoding: "utf8",
  transform(c, _e, cb) {
    out.push(typeof c === "string" ? c : c.toString());
    cb();
  },
});
t.on("finish", () => console.log("joined:", out.join("")));
t.write("hello", "utf8");
t.end();
