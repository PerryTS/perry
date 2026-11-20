import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const emitter = new EventEmitter();
d.add(emitter);
console.log(String(d.remove(emitter)));
console.log(d.members.length, emitter.domain === null);
console.log(Object.prototype.propertyIsEnumerable.call(emitter, "domain"));
