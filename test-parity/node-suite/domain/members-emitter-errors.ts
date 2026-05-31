import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const ee = new EventEmitter();

d.on("error", (err: any) => {
  console.log(
    "domain error:",
    err.message,
    err.domain === d,
    err.domainThrown,
    typeof err.domainBound,
    err.domainEmitter === ee,
  );
});

d.add(ee);
console.log("members after add:", d.members.includes(ee), ee.domain === d, d.members.length);
console.log("emit returned:", ee.emit("error", new Error("emitter boom")));
d.remove(ee);
console.log("members after remove:", d.members.includes(ee), ee.domain, d.members.length);
