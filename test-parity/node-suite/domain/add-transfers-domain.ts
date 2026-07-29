import domain from "node:domain";
import { EventEmitter } from "node:events";

const first = domain.create();
const second = domain.create();
const emitter = new EventEmitter();
first.add(emitter);
second.add(emitter);
console.log(
  first.members.length,
  second.members.length,
  emitter.domain === second,
);
second.remove(emitter);
