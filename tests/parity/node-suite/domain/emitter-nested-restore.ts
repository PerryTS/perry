import domain from "node:domain";
import { EventEmitter } from "node:events";

const outer = domain.create();
const routed = domain.create();
const emitter = new EventEmitter();
routed.add(emitter);
routed.on("error", () => console.log("handler", domain.active === outer));
outer.enter();
console.log(emitter.emit("error", new Error("nested")));
console.log("restored", domain.active === outer, (domain as any)._stack.length);
outer.exit();
routed.remove(emitter);
