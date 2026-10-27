import {EventEmitter} from "node:events";
const emitter=new EventEmitter();
emitter.once("hws",value=>console.log(value));emitter.emit("hws",42);
console.log(emitter.listenerCount("hws"));
console.log(new Error("ordinary").message);
