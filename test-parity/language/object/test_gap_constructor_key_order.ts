// #12327: constructor assignments are property stores, not field declarations.
import { EventEmitter } from "node:events";
function show(label, obj) {
  console.log(label, Object.keys(obj).join(","));
  const keys = []; for (const key in obj) keys.push(key);
  console.log(label, keys.join(","));
  console.log(label, JSON.stringify(obj));
}
class Cell {
  constructor(o) { this.setOptions(o); this.x = null; this.y = null; }
  setOptions(o) { this.options = o; this.colSpan = 1; this.rowSpan = 1; }
}
show("method", new Cell(1));
console.log("method-values", new Cell(7).options, new Cell(7).x);
class Cmd extends EventEmitter {
  constructor() { super(); this.commands = []; this.name2 = "c"; }
}
show("native", new Cmd());
class Cond {
  constructor(f) { if (f) { this.b = 2; } this.a = 1; }
}
show("true", new Cond(true));
show("false", new Cond(false));
console.log("conditional-values", new Cond(true).a, new Cond(false).a);
class Twice {
  constructor() { this.before(); this.a = 1; this.b = 2; this.a = 3; }
  before() { this.first = 0; }
}
show("twice", new Twice());
class Readd {
  constructor() {
    this.before(); this.a = 1; this.b = 2;
    delete this.a; this.a = 3;
  }
  before() { this.first = 0; }
}
show("readd", new Readd());
class Base {
  constructor() { this.before(); this.base = 1; }
  before() { this.first = 0; }
}
class Child extends Base {
  constructor() { super(); this.middle(); this.child = 2; this.base = 3; }
  middle() { this.extra = 4; }
}
show("subclass", new Child());
console.log("subclass-values", new Child().base, new Child().child);
// A store of undefined still creates a key; a skipped store never does.
class Presence {
  constructor(f) {
    console.log("birth", Object.keys(this).join(","), "a" in this);
    if (f) this.b = undefined;
    this.a = undefined;
  }
}
show("undefined", new Presence(true));
show("skipped", new Presence(false));
class Sequence {
  constructor() { this.before(); this.a = this.b = 1, this.c = 2; }
  before() { this.first = 0; }
}
show("sequence", new Sequence());
const Expression = class {
  constructor() { this.before(); this.a = 1; }
  before() { this.first = 0; }
};
show("expression", new Expression());
class DeclaredChild extends Base {
  child = 2;
  constructor() { super(); this.last = 3; }
}
show("declared-subclass", new DeclaredChild());
class SimpleBase { constructor() { this.base = 1; } }
class SimpleChild extends SimpleBase { child = 2; }
show("mixed-subclass", new SimpleChild());
// Initializers may observe which later fields have actually been defined.
class DeclaredOrder {
  a = Object.keys(this).join(",") + ":" + ("b" in this);
  b = Object.keys(this).join(",");
}
show("declared-order", new DeclaredOrder());
class ReversedDeclared {
  a: number;
  b: number;
  constructor(a: number, b: number) { this.b = b; this.a = a; }
}
show("declared-reversed", new ReversedDeclared(1, 2));
class StraightDeclared {
  a: number;
  b: number;
  constructor(a: number, b: number) { this.a = a; this.b = b; }
}
show("declared-fused", new StraightDeclared(1, 2));
function makeCaptured(value) {
  return class extends SimpleBase {
    child = value;
    seen = Object.keys(this).join(",");
  };
}
const Captured = makeCaptured(2);
show("captured-subclass", new Captured());
show("captured-again", new Captured());
const OtherCaptured = makeCaptured(3);
show("captured-evaluation", new OtherCaptured());
show("reflect", Reflect.construct(DeclaredOrder, []));
function construct(C) { return new C(); }
show("class-ref", construct(DeclaredOrder));
show("object-newtarget", Reflect.construct(Object, [], DeclaredOrder));
class Assigned { constructor(value) { this.a = value; } }
show("setter-warm", new Assigned(1));
Object.defineProperty(Assigned.prototype, "a", {
  set(value) { this.before = value; }, get() { return 9; }, configurable: true,
});
show("setter-installed", new Assigned(2));
delete Assigned.prototype.a;
show("setter-removed", new Assigned(3));
class Private {
  #secret = 4;
  a = 1;
  b = this.#secret;
}
show("private-fields", new Private());
show("private-fields-again", new Private());
