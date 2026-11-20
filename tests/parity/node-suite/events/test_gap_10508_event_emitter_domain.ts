// #10508: an emitter is an ordinary object, so `domain.add(ee)` binds it the
// way node's domain module does — an own, non-enumerable `domain` property —
// and an unhandled 'error' then goes to the domain with `er.domainEmitter`,
// `er.domain` and `er.domainThrown` set. `domain.remove(ee)` unbinds it.
import { EventEmitter } from "node:events";
import * as domain from "node:domain";

const d = domain.create();
d.on("error", (er: any) =>
  console.log("domain caught", er.message, er.domainEmitter === e, er.domain === d, er.domainThrown),
);
const e = new EventEmitter();
d.add(e);
console.log("bound", (e as any).domain === d, Object.keys(e).join(","));
e.emit("error", new Error("boom"));
d.remove(e);
console.log("unbound", (e as any).domain);
try {
  e.emit("error", new Error("after"));
} catch (er: any) {
  console.log("thrown", er.message);
}
