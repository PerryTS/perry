import domain from "node:domain";
import { EventEmitter } from "node:events";

const d = domain.create();
console.log(d instanceof EventEmitter);
console.log(
  Object.getPrototypeOf(domain.Domain.prototype) === EventEmitter.prototype,
);
