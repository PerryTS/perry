// #10477 fixture: function constructors (not classes) exported from a module,
// checked with `instanceof` by the importer.
import { inherits } from "node:util";

// Prototype untouched.
export function Plain(this: any, v: number) {
  this.v = v;
}
Plain.prototype.get = function (this: any) {
  return this.v;
};

// Prototype replaced wholesale (the bignumber.js / decimal.js shape).
export function Swapped(this: any, v: number) {
  this.v = v;
}
Swapped.prototype = {
  constructor: Swapped,
  get(this: any) {
    return this.v;
  },
};

// Controls: real classes keep the static class-id check.
export class Klass {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
export class SubKlass extends Klass {}
export const ExprKlass = class {
  w = 2;
};

// ES5 inheritance, both idioms.
export function Base(this: any) {
  this.base = true;
}
export function Inherited(this: any) {
  Base.call(this);
}
inherits(Inherited, Base);
export function Linked(this: any) {}
Object.setPrototypeOf(Linked.prototype, Base.prototype);

// An own Symbol.hasInstance overrides the prototype walk.
export function Duck() {}
Object.defineProperty(Duck, Symbol.hasInstance, {
  value: (x: any) => !!x && x.quack === true,
});

// User constructors that share a name with a builtin the static path maps to a
// reserved class id.
export function Headers(this: any) {
  this.h = 1;
}
export function EventEmitter(this: any) {
  this.e = 1;
}
export function Stream(this: any) {
  this.s = 1;
}

// Factory-built constructor held in an exported const (decimal.js `clone()`).
function factory() {
  function Made(this: any, v: number): any {
    if (!(this instanceof Made)) return new (Made as any)(v);
    this.v = v;
  }
  Made.prototype = { constructor: Made };
  return Made;
}
export const MadeConst: any = factory();

// A live binding: the importer must read the current value.
export let Rebound: any = function First(this: any) {};
export function rebind() {
  Rebound = function Second(this: any) {};
}

export const notCallable = { prototype: {} };

export function makePlain(v: number) {
  return new (Plain as any)(v);
}
export function makeSwapped(v: number) {
  return new (Swapped as any)(v);
}
export function isPlainHere(x: unknown) {
  return x instanceof Plain;
}
