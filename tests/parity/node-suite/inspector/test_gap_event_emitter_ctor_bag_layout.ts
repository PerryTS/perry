// node:inspector and node:cluster read events.EventEmitter from the runtime, with no events import.
import { Session } from "node:inspector";
import { Worker } from "node:cluster";
const EE = Object.getPrototypeOf(Session.prototype).constructor;
console.log(EE.name, typeof EE.listenerCount, typeof new Session().on, typeof Worker.prototype.on);
