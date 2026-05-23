import { Transform } from "node:stream";
// A Transform converts each chunk through its transform() callback.
const upper = new Transform({
  transform(chunk, _enc, cb) {
    cb(null, String(chunk).toUpperCase());
  },
});
const out: string[] = [];
upper.on("data", (c) => out.push(String(c)));
upper.on("end", () => console.log("joined:", out.join("")));
upper.write("hello ");
upper.write("world");
upper.end();
