import { Transform } from "node:stream";
// Inside transform(), `this` should refer to the Transform instance,
// allowing this.push() to enqueue output.
const t = new Transform({
  transform(chunk, _enc, cb) {
    console.log("this is Transform:", this instanceof Transform);
    this.push(String(chunk).toUpperCase());
    cb();
  },
});
t.on("data", () => {});
t.write("hi");
t.end();
