import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const emitter = new EventEmitter();
d.add(emitter);
d.add(emitter);
console.log(d.members.length, d.members[0] === emitter, emitter.domain === d);
d.remove(emitter);
