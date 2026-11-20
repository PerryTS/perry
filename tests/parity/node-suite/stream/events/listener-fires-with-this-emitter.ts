import { Readable } from "node:stream";
// 'data' listener with `this` — binds to the emitter (in non-arrow fn).
const r = new Readable({ read() {} });
let thisIsEmitter = false;
r.on("data", function (this: any) {
  thisIsEmitter = this === r;
});
r.push("x");
r.push(null);
r.on("end", () => console.log("this===r:", thisIsEmitter));
