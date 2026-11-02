// A CommonJS EventEmitter subclass (`class extends require("events")
// .EventEmitter` and util.inherits) whose instances use on/once/emit/off.
import * as em from "./cjs2_shapes/emitter.cjs";

const m: any = (em as any).default ?? em;
const bus = new m.Bus();
const got: string[] = [];
const h = (v: number) => got.push("a" + v);
bus.on("t", h);
bus.once("t", (v: number) => got.push("once" + v));
console.log(bus.publish("t", 1), bus.publish("t", 2), bus.publish("none", 3));
bus.off("t", h);
console.log(bus.publish("t", 4), bus.listenerCount("t"));
console.log(got.join(","));
const l = new m.Legacy();
l.on("data", (v: string) => l.seen.push(v.toUpperCase()));
l.feed("x");
l.feed("y");
console.log(l.seen.join(""), l instanceof m.Legacy, typeof l.emit);
