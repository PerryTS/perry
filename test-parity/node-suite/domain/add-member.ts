import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
const emitter = new EventEmitter();
console.log(String(d.add(emitter)));
const descriptor = Object.getOwnPropertyDescriptor(emitter, "domain")!;
console.log(emitter.domain === d, d.members[0] === emitter, d.members.length);
console.log(
  descriptor?.enumerable,
  descriptor?.configurable,
  descriptor?.writable,
);
d.remove(emitter);
