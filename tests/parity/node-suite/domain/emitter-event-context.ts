import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const emitter = new EventEmitter();
d.add(emitter);
emitter.on("data", function (value) {
  console.log(this === emitter, domain.active === d, value);
});
console.log(emitter.emit("data", "payload"), String(domain.active));
d.remove(emitter);
