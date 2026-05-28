import { Writable } from "node:stream";
// end(cb) — cb fires after pending writes flush, before 'finish' listeners.
const order: string[] = [];
const w = new Writable({
  write(_c, _e, cb) {
    setImmediate(() => {
      order.push("write");
      cb();
    });
  },
});
w.on("finish", () => order.push("finish"));
w.write("a");
w.end(() => {
  order.push("end-cb");
  console.log("order:", order.join(","));
});
