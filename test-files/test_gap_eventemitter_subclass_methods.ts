// A class extending EventEmitter: construction with many fields, the
// inherited on/emit/once/off surface, its own methods called in a loop, an
// overridden emit reaching super.emit, and an own property shadowing a class
// method after the call site has warmed up.
import { EventEmitter } from "node:events";

class Query extends EventEmitter {
  text: any; values: any; rows: any; name: any; binary: any; portal: any; callback: any;
  constructor(i: number) {
    super();
    this.text = "SELECT " + i; this.values = [i]; this.rows = undefined; this.name = undefined;
    this.binary = false; this.portal = ""; this.callback = null;
  }
  requiresPreparation(): boolean { return this.values.length > 0; }
  label(): string { return "query:" + this.text; }
}

class Logged extends Query {
  log: string[] = [];
  emit(event: string | symbol, ...args: any[]): boolean {
    this.log.push(String(event));
    return super.emit(event, ...args);
  }
}

let built = 0;
const keep: Query[] = [];
for (let i = 0; i < 2000; i++) {
  const q = new Query(i);
  keep[i & 63] = q;
  built += q.values[0] & 1;
}
console.log("built", built, keep[5].text);

const q = new Query(7);
let got = 0;
const onMessage = (m: number) => { got += m; };
q.on("message", onMessage);
q.once("message", (m: number) => { got += 1000 * m; });
let emitted = 0;
for (let i = 0; i < 1000; i++) if (q.emit("message", 1)) emitted++;
console.log("emit", emitted, got, q.listenerCount("message"));
q.off("message", onMessage);
console.log("after off", q.emit("message", 5), q.listenerCount("message"), got);

let prep = 0;
for (let i = 0; i < 5000; i++) if (q.requiresPreparation()) prep++;
console.log("method", prep, q.label());

const l = new Logged(3);
let seen = 0;
l.on("x", () => { seen++; });
l.emit("x"); l.emit("x"); l.emit("y");
console.log("override", seen, l.log.join(","), l.label());

// An own property installed after the site warmed shadows the class method.
const shadow = new Query(9);
let s = "";
for (let i = 0; i < 100; i++) s = shadow.label();
(shadow as any).label = () => "own-label";
console.log("shadow", s, shadow.label(), q.label());
