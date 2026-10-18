// Evaluated heritage keeps its value flow and evaluates each expression once.
class AWEBaseA {
  value: number;
  constructor(value: number) { this.value = value; }
  read() { return "A" + this.value; }
  static label() { return "A"; }
}
class AWEBaseB {
  padding: number;
  value: number;
  constructor(value: number) { this.padding = 99; this.value = value; }
  read() { return "B" + this.value; }
  static label() { return "B"; }
}
function conditional(cond: boolean) {
  return class extends (cond ? AWEBaseA : AWEBaseB) {
    static stamp = 1;
    read() { return super.read() + ":child"; }
    static label() { return super.label() + ":child"; }
  };
}
const AWEFirst = conditional(true);
const AWESecond = conditional(false);
console.log(new AWEFirst(3).read(), new AWESecond(4).read());
console.log(AWEFirst.label(), AWESecond.label());
function mixin(Parent: any) {
  return class extends Parent {
    static marker = 2;
    mixed() { return this.read() + ":mixed"; }
  };
}
function mixedChild() { return class extends mixin(AWEBaseA) { static marker = 3; }; }
const AWEMixed = mixedChild();
console.log(new AWEMixed(5).mixed());
const aweEvents: string[] = [];
function evaluatedParent() { aweEvents.push("parent"); return AWEBaseA; }
function sideEffects() {
  return class extends evaluatedParent() {
    static marker = (aweEvents.push("static"), 4);
  };
}
const AWESide = sideEffects();
console.log(new AWESide(6).read(), aweEvents.join(","));
(globalThis as any).__aweParent = AWEBaseB;
function unknownParent() { return (globalThis as any).__aweParent; }
function unknownChild() { return class extends unknownParent() { static marker = 5; }; }
const AWEUnknown = unknownChild();
console.log(new AWEUnknown(7).read());
delete (globalThis as any).__aweParent;
function nullChild() {
  return class extends null {
    static marker = 6;
    constructor() { return Object.create(new.target.prototype); }
    read() { return "null"; }
  };
}
const AWENull = nullChild();
console.log(Object.getPrototypeOf(AWENull.prototype) === null, new AWENull().read());
