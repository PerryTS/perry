import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
let emitter: EventEmitter;
d.run(() => {
  emitter = new EventEmitter();
  console.log(emitter.domain === d, d.members.includes(emitter));
});
console.log(emitter!.domain === d, String(domain.active));
