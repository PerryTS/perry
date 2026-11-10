import { Writable } from "node:stream";
// end(chunk) — the chunk is written BEFORE 'finish' fires.
const order: string[] = [];
const w = new Writable({
  write(c, _e, cb) {
    order.push("write:" + String(c));
    cb();
  },
});
w.on("finish", () => {
  order.push("finish");
  console.log("order:", order.join(","));
  console.log("write before finish:", order.indexOf("write:end-chunk") < order.indexOf("finish"));
});
w.end("end-chunk");
