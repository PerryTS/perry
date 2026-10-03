// Class fields are DefineField (CreateDataPropertyOrThrow), never [[Set]]:
// an inherited setter must not run, the field becomes an ordinary writable,
// enumerable, configurable own property, and a receiver that cannot take the
// property throws. Exercised on instances that left their construction layout
// before the fields run (an EventEmitter base installs its surface first).
import { EventEmitter } from "node:events";

const log: string[] = [];

class Mid extends EventEmitter {
  set tag(v: unknown) { log.push("Mid.tag setter " + String(v)); }
  get tag(): unknown { return "Mid.tag getter"; }
}
class Leaf extends Mid {
  tag: any = 1;
  count = 0;
  label = "leaf";
  bump(): number { return ++this.count; }
}

let sum = 0;
for (let i = 0; i < 500; i++) {
  const l = new Leaf();
  sum += l.bump() + (l.tag as number);
}
const leaf = new Leaf();
const d = Object.getOwnPropertyDescriptor(leaf, "tag")!;
console.log("sum", sum, "setter calls", log.length);
console.log("own tag", leaf.tag, d.value, d.writable, d.enumerable, d.configurable);
leaf.tag = 7;
console.log("after assign", leaf.tag, log.length, leaf.label);
let fired = 0;
leaf.on("x", () => { fired++; });
leaf.emit("x");
console.log("emitter still works", fired, leaf.listenerCount("x"));

// A base constructor that already created the key: the field redefines it.
class Pre extends EventEmitter {
  constructor() { super(); (this as any).shared = "from base"; }
}
class PreField extends Pre { shared = "from field"; other = 2; }
const pf = new PreField();
console.log("redefine", pf.shared, pf.other, Object.getOwnPropertyDescriptor(pf, "shared")!.enumerable);

// A base constructor returning an object that cannot take the property.
class Frozen { constructor() { return Object.freeze({ k: 1 }); } }
class FrozenField extends Frozen { extra = 1; }
try { new FrozenField(); console.log("frozen", "no error"); }
catch (e) { console.log("frozen", e instanceof TypeError); }

class Locked {
  constructor() {
    const o = {};
    Object.defineProperty(o, "fixed", { value: 0, writable: false, configurable: false });
    return o;
  }
}
class LockedField extends Locked { fixed = 1; }
try { new LockedField(); console.log("non-configurable", "no error"); }
catch (e) { console.log("non-configurable", e instanceof TypeError); }

// Moving collections while instances are built.
const keep: Leaf[] = [];
let total = 0;
for (let i = 0; i < 5000; i++) {
  const l = new Leaf();
  keep[i & 255] = l;
  total += l.bump();
  if ((i & 1023) === 0) { const junk = new Array(2000).fill(i); total += junk.length & 1; }
}
console.log("churn", total, keep[3].label, keep[3].tag);
