// E21: Error.captureStackTrace writes `stack`; a native EventEmitter parent.
// WPFACTS: fact F1 readonly Oops.stack = 0
import { EventEmitter } from "events";
class Oops {
  stack: string = "";
  code: number = 7;
  constructor() {
    Error.captureStackTrace(this);
  }
}
class Bus extends EventEmitter {
  hits: number = 0;
}
const o = new Oops();
const b = new Bus();
b.on("x", (n: number) => { b.hits += n; });
b.emit("x", 3);
console.log(typeof o.stack, o.code, b.hits);
