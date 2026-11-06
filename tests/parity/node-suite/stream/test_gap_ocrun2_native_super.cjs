// Filesystem prototype materialization must not change native super lookup.
const { statSync } = require("node:fs");
const Stream = require("node:stream");
Object.getPrototypeOf(statSync("."));

class Forward extends Stream {
  on(event, listener) {
    return super.on(event, listener);
  }
}
const emitter = new Forward();
let total = 0;
console.log(emitter.on("data", (n) => { total += n; }) === emitter);
emitter.emit("data", 3);
console.log(total);
