import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const emitter = new EventEmitter();
d.add(emitter);
d.on("error", (error: any) => {
  console.log(
    error.message,
    error.domain === d,
    error.domainEmitter === emitter,
  );
  console.log(error.domainThrown, error.domainBound, domain.active);
});
console.log(emitter.emit("error", new Error("routed")));
d.remove(emitter);
