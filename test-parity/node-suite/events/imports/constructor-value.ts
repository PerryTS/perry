import { EventEmitter } from "node:events";
const EE = EventEmitter;
const Bound = EE.bind(null);
const emitter = new Bound();
emitter.on("value", value => console.log("bound", value));
emitter.emit("value", 4);
class Bus extends EE {}
const bus = new Bus();
bus.on("value", value => console.log("subclass", value));
bus.emit("value", 7);
console.log(bus instanceof EE, bus instanceof Bus, emitter instanceof EE);
